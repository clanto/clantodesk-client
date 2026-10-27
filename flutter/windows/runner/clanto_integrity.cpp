// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Integrità dei binari (CLANTO.md, «Firma del codice»): ogni .exe e .dll
// accanto all'eseguibile deve avere una firma Authenticode valida di Clanto,
// riconosciuta dall'EKU dell'identità Azure Trusted Signing, oppure di
// Microsoft (componenti ridistribuiti, anche firmati tramite catalogo).
// Nessuna rete: niente controllo di revoca, solo URL già in cache.

#include "clanto_integrity.h"

#include <windows.h>
#include <bcrypt.h>
#include <mscat.h>
#include <softpub.h>
#include <wincrypt.h>
#include <wintrust.h>

#include <cstring>
#include <cwchar>
#include <vector>

#pragma comment(lib, "wintrust.lib")
#pragma comment(lib, "crypt32.lib")

namespace {

struct Signer {
  bool clanto = false;
  bool microsoft = false;
};

void InspectLeaf(PCCERT_CONTEXT cert, const char* eku, Signer* signer) {
  DWORD size = 0;
  if (CertGetEnhancedKeyUsage(cert, CERT_FIND_EXT_ONLY_ENHKEY_USAGE_FLAG,
                              nullptr, &size) &&
      size > 0) {
    std::vector<BYTE> buffer(size);
    auto usage = reinterpret_cast<PCERT_ENHKEY_USAGE>(buffer.data());
    if (CertGetEnhancedKeyUsage(cert, CERT_FIND_EXT_ONLY_ENHKEY_USAGE_FLAG,
                                usage, &size)) {
      for (DWORD i = 0; i < usage->cUsageIdentifier; i++) {
        if (std::strcmp(usage->rgpszUsageIdentifier[i], eku) == 0) {
          signer->clanto = true;
        }
      }
    }
  }
  wchar_t name[256] = {0};
  CertGetNameStringW(cert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, nullptr, name,
                     256);
  if (std::wcscmp(name, L"Microsoft Windows") == 0 ||
      std::wcscmp(name, L"Microsoft Corporation") == 0) {
    signer->microsoft = true;
  }
}

// WinVerifyTrust e, se la firma è valida, il certificato del firmatario.
LONG Verify(WINTRUST_DATA* data, const char* eku, Signer* signer) {
  GUID action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
  data->cbStruct = sizeof(WINTRUST_DATA);
  data->dwUIChoice = WTD_UI_NONE;
  data->fdwRevocationChecks = WTD_REVOKE_NONE;
  data->dwProvFlags = WTD_CACHE_ONLY_URL_RETRIEVAL;
  data->dwStateAction = WTD_STATEACTION_VERIFY;
  HWND no_ui = static_cast<HWND>(INVALID_HANDLE_VALUE);
  LONG status = WinVerifyTrust(no_ui, &action, data);
  if (status == ERROR_SUCCESS) {
    CRYPT_PROVIDER_DATA* provider =
        WTHelperProvDataFromStateData(data->hWVTStateData);
    CRYPT_PROVIDER_SGNR* sgnr =
        provider ? WTHelperGetProvSignerFromChain(provider, 0, FALSE, 0)
                 : nullptr;
    CRYPT_PROVIDER_CERT* leaf =
        sgnr ? WTHelperGetProvCertFromChain(sgnr, 0) : nullptr;
    if (leaf && leaf->pCert) {
      InspectLeaf(leaf->pCert, eku, signer);
    }
  }
  data->dwStateAction = WTD_STATEACTION_CLOSE;
  WinVerifyTrust(no_ui, &action, data);
  return status;
}

// Funzioni SHA-256 del catalogo (Windows 8+), caricate da wintrust.dll.
typedef BOOL(WINAPI* AcquireContext2Fn)(HCATADMIN*, const GUID*, PCWSTR,
                                        PCCERT_STRONG_SIGN_PARA, DWORD);
typedef BOOL(WINAPI* CalcHash2Fn)(HCATADMIN, HANDLE, DWORD*, BYTE*, DWORD);

// File senza firma propria: vale solo se è nel catalogo di Microsoft.
bool CatalogSignedByMicrosoft(const std::wstring& path, const char* eku) {
  HMODULE wintrust = GetModuleHandleW(L"wintrust.dll");
  auto acquire2 = wintrust ? reinterpret_cast<AcquireContext2Fn>(
                                 GetProcAddress(wintrust, "CryptCATAdminAcquireContext2"))
                           : nullptr;
  auto hash2 = wintrust ? reinterpret_cast<CalcHash2Fn>(GetProcAddress(
                              wintrust, "CryptCATAdminCalcHashFromFileHandle2"))
                        : nullptr;
  if (!acquire2 || !hash2) {
    return false;
  }
  HANDLE file = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ,
                            nullptr, OPEN_EXISTING, 0, nullptr);
  if (file == INVALID_HANDLE_VALUE) {
    return false;
  }
  bool trusted = false;
  HCATADMIN admin = nullptr;
  if (acquire2(&admin, nullptr, BCRYPT_SHA256_ALGORITHM, nullptr, 0)) {
    DWORD size = 0;
    hash2(admin, file, &size, nullptr, 0);
    std::vector<BYTE> hash(size);
    if (size > 0 && hash2(admin, file, &size, hash.data(), 0)) {
      HCATINFO catalog =
          CryptCATAdminEnumCatalogFromHash(admin, hash.data(), size, 0, nullptr);
      if (catalog) {
        CATALOG_INFO info = {};
        info.cbStruct = sizeof(info);
        if (CryptCATCatalogInfoFromContext(catalog, &info, 0)) {
          std::wstring tag;
          for (BYTE b : hash) {
            wchar_t hex[3];
            swprintf(hex, 3, L"%02X", b);
            tag += hex;
          }
          WINTRUST_CATALOG_INFO member = {};
          member.cbStruct = sizeof(member);
          member.pcwszCatalogFilePath = info.wszCatalogFile;
          member.pcwszMemberFilePath = path.c_str();
          member.pcwszMemberTag = tag.c_str();
          member.pbCalculatedFileHash = hash.data();
          member.cbCalculatedFileHash = size;
          member.hMemberFile = file;
          member.hCatAdmin = admin;
          WINTRUST_DATA data = {};
          data.dwUnionChoice = WTD_CHOICE_CATALOG;
          data.pCatalog = &member;
          Signer signer;
          trusted = Verify(&data, eku, &signer) == ERROR_SUCCESS &&
                    signer.microsoft;
        }
        CryptCATAdminReleaseCatalogContext(admin, catalog, 0);
      }
    }
    CryptCATAdminReleaseContext(admin, 0);
  }
  CloseHandle(file);
  return trusted;
}

bool FileTrusted(const std::wstring& path, const char* eku) {
  WINTRUST_FILE_INFO info = {};
  info.cbStruct = sizeof(info);
  info.pcwszFilePath = path.c_str();
  WINTRUST_DATA data = {};
  data.dwUnionChoice = WTD_CHOICE_FILE;
  data.pFile = &info;
  Signer signer;
  LONG status = Verify(&data, eku, &signer);
  if (status == ERROR_SUCCESS) {
    return signer.clanto || signer.microsoft;
  }
  return status == TRUST_E_NOSIGNATURE && CatalogSignedByMicrosoft(path, eku);
}

bool IsBinary(const wchar_t* name) {
  const wchar_t* dot = std::wcsrchr(name, L'.');
  return dot && (_wcsicmp(dot, L".exe") == 0 || _wcsicmp(dot, L".dll") == 0);
}

std::wstring ExecutableDir() {
  std::vector<wchar_t> buffer(MAX_PATH);
  for (;;) {
    DWORD len = GetModuleFileNameW(nullptr, buffer.data(),
                                   static_cast<DWORD>(buffer.size()));
    if (len == 0) {
      return L"";
    }
    if (len < buffer.size()) {
      std::wstring path(buffer.data(), len);
      size_t slash = path.find_last_of(L"\\/");
      return slash == std::wstring::npos ? L"" : path.substr(0, slash);
    }
    buffer.resize(buffer.size() * 2);
  }
}

void ReportEvent61180(const std::wstring& files) {
  HANDLE source = RegisterEventSourceW(nullptr, L"ClantoDesk");
  if (!source) {
    return;
  }
  std::wstring text = L"ClantoDesk binaries not signed by Clanto:\n" + files;
  LPCWSTR strings[1] = {text.c_str()};
  ReportEventW(source, EVENTLOG_ERROR_TYPE, 0, 61180, nullptr, 1, 0, strings,
               nullptr);
  DeregisterEventSource(source);
}

}  // namespace

std::wstring ClantoTamperedFiles(const std::wstring& dir, const char* eku) {
  std::wstring tampered;
  if (eku == nullptr || eku[0] == '\0' || dir.empty()) {
    return tampered;
  }
  WIN32_FIND_DATAW entry;
  HANDLE find = FindFirstFileW((dir + L"\\*").c_str(), &entry);
  if (find == INVALID_HANDLE_VALUE) {
    return tampered;
  }
  do {
    if ((entry.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) == 0 &&
        IsBinary(entry.cFileName) &&
        !FileTrusted(dir + L"\\" + entry.cFileName, eku)) {
      tampered += entry.cFileName;
      tampered += L"\n";
    }
  } while (FindNextFileW(find, &entry));
  FindClose(find);
  return tampered;
}

bool ClantoIntegrityOk() {
#ifdef CLANTO_SIGNING_EKU
  std::wstring tampered = ClantoTamperedFiles(ExecutableDir(), CLANTO_SIGNING_EKU);
  if (tampered.empty()) {
    return true;
  }
  ReportEvent61180(tampered);
  DWORD session = 0;
  // Sessione 0 (servizio) o CI: nessuno a cui mostrare l'errore.
  bool ci = GetEnvironmentVariableW(L"CI", nullptr, 0) > 0;
  if (!ci && ProcessIdToSessionId(GetCurrentProcessId(), &session) &&
      session != 0) {
    bool italian = PRIMARYLANGID(GetUserDefaultUILanguage()) == LANG_ITALIAN;
    std::wstring text =
        // Accenti come escape: il sorgente non dipende dalla codepage del compilatore.
        italian ? L"ClantoDesk \u00E8 stato manomesso o danneggiato: questi file "
                  L"non sono firmati da Clanto.\n\n" +
                      tampered +
                      L"\nPer sicurezza ClantoDesk non pu\u00F2 essere avviato. "
                      L"Reinstallalo scaricandolo da clanto.it."
                : L"ClantoDesk has been tampered with or damaged: these files "
                  L"are not signed by Clanto.\n\n" +
                      tampered +
                      L"\nFor security ClantoDesk cannot start. Reinstall it "
                      L"from clanto.it.";
    MessageBoxW(nullptr, text.c_str(),
                italian ? L"Eseguibile manomesso" : L"Tampered executable",
                MB_OK | MB_ICONERROR | MB_TOPMOST);
  }
  return false;
#else
  return true;
#endif
}
