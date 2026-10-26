// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Firma Authenticode incorporata di un file, con `WinVerifyTrust`: nessuna
//! rete (niente revoca, solo URL già in cache). Restituisce il nome e gli EKU
//! del certificato del firmatario per riconoscere l'identità Clanto.

use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::Path};
use windows::{
    core::{PCWSTR, PSTR},
    Win32::{
        Foundation::{HWND, INVALID_HANDLE_VALUE},
        Security::{
            Cryptography::{
                CertGetEnhancedKeyUsage, CertGetNameStringW, CERT_CONTEXT, CERT_FIND_EXT_ONLY_ENHKEY_USAGE_FLAG,
                CERT_NAME_SIMPLE_DISPLAY_TYPE, CTL_USAGE,
            },
            WinTrust::{
                WTHelperGetProvCertFromChain, WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData,
                WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
                WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
                WTD_STATEACTION_VERIFY, WTD_UI_NONE,
            },
        },
    },
};

/// Firmatario di un file con firma valida.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Signer {
    /// Nome semplice del certificato (CN).
    pub name: String,
    /// OID degli EKU del certificato.
    pub ekus: Vec<String>,
}

impl Signer {
    pub fn has_eku(&self, eku: &str) -> bool {
        self.ekus.iter().any(|e| e == eku)
    }
}

unsafe fn leaf_signer(cert: *const CERT_CONTEXT) -> Signer {
    let mut signer = Signer::default();
    let mut size = 0u32;
    if CertGetEnhancedKeyUsage(cert, CERT_FIND_EXT_ONLY_ENHKEY_USAGE_FLAG.0, None, &mut size).is_ok() && size > 0 {
        let mut buffer = vec![0u8; size as usize];
        let usage = buffer.as_mut_ptr() as *mut CTL_USAGE;
        if CertGetEnhancedKeyUsage(cert, CERT_FIND_EXT_ONLY_ENHKEY_USAGE_FLAG.0, Some(usage), &mut size).is_ok() {
            let usage = &*usage;
            for i in 0..usage.cUsageIdentifier as usize {
                let oid: PSTR = *usage.rgpszUsageIdentifier.add(i);
                if let Ok(oid) = oid.to_string() {
                    signer.ekus.push(oid);
                }
            }
        }
    }
    let mut name = [0u16; 256];
    let len = CertGetNameStringW(cert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, Some(&mut name));
    signer.name = String::from_utf16_lossy(&name[..(len as usize).saturating_sub(1)]);
    signer
}

/// Firmatario di `path` se la firma Authenticode incorporata è valida,
/// altrimenti l'errore di `WinVerifyTrust` (es. `0x800B0100` senza firma).
pub fn embedded_signer(path: &Path) -> Result<Signer, String> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        ..Default::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let no_ui = HWND(INVALID_HANDLE_VALUE.0);
    unsafe {
        let status = WinVerifyTrust(no_ui, &mut action, &mut data as *mut _ as *mut c_void);
        let result = if status == 0 {
            let provider = WTHelperProvDataFromStateData(data.hWVTStateData);
            let sgnr = if provider.is_null() {
                std::ptr::null_mut()
            } else {
                WTHelperGetProvSignerFromChain(provider, 0, false, 0)
            };
            let cert = if sgnr.is_null() {
                std::ptr::null_mut()
            } else {
                WTHelperGetProvCertFromChain(sgnr, 0)
            };
            if cert.is_null() || (*cert).pCert.is_null() {
                Err("signer certificate not available".to_owned())
            } else {
                Ok(leaf_signer((*cert).pCert))
            }
        } else {
            Err(format!("signature not valid (0x{:08X})", status as u32))
        };
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(no_ui, &mut action, &mut data as *mut _ as *mut c_void);
        result
    }
}
