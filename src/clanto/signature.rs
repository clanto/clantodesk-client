// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Firma del codice (CLANTO.md, «Firma del codice»).
//!
//! - **Windows.** L'identità Clanto è l'EKU di Azure Trusted Signing
//!   (`CLANTO_SIGNING_EKU` al build): stabile anche se il certificato ruota e
//!   impossibile da ottenere per un'altra società con lo stesso nome. L'exe
//!   verifica all'avvio ogni `.exe`/`.dll` della propria cartella
//!   (`flutter/windows/runner/clanto_integrity.cpp`); qui `librustdesk.dll`
//!   verifica a sua volta l'exe, così toccare uno solo dei due non basta.
//!   Gli aggiornamenti si installano solo se firmati con lo stesso EKU.
//! - **macOS.** Bundle firmato dal team Apple Clanto (`CLANTO_SIGNING_TEAM` al
//!   build), verificato con Security.framework: all'avvio del servizio e
//!   dell'interfaccia, e sugli aggiornamenti.
//!
//! Senza le variabili al build (sviluppo) non si verifica nulla. Chi è
//! amministratore può togliere anche questi controlli: il blocco vero sono le
//! regole App Control di Windows sull'EKU.

#[cfg(any(windows, target_os = "macos"))]
use hbb_common::{bail, log, ResultType};
#[cfg(any(windows, target_os = "macos"))]
use std::path::Path;

/// EKU dell'identità Clanto in Azure Trusted Signing.
#[cfg(windows)]
fn signing_eku() -> Option<&'static str> {
    option_env!("CLANTO_SIGNING_EKU").filter(|eku| !eku.is_empty())
}

/// Team ID Apple del certificato Developer ID.
#[cfg(target_os = "macos")]
fn signing_team() -> Option<&'static str> {
    option_env!("CLANTO_SIGNING_TEAM").filter(|team| !team.is_empty())
}

/// Windows: `path` ha una firma Authenticode valida con l'EKU Clanto.
#[cfg(windows)]
pub fn verify_windows_file(path: &Path) -> ResultType<()> {
    let Some(eku) = signing_eku() else {
        return Ok(());
    };
    let signer = clanto_win::trust::embedded_signer(path)
        .map_err(|err| hbb_common::anyhow::anyhow!("{}: {err}", path.display()))?;
    if !signer.has_eku(eku) {
        bail!("{} is not signed by Clanto (signer {:?})", path.display(), signer.name);
    }
    Ok(())
}

/// Windows, installazione e aggiornamento: comandi che cancellano dalla cartella
/// di installazione gli `.exe`/`.dll` che il nuovo pacchetto non contiene più
/// (es. plugin Flutter tolti). Rimasti lì, non firmati con l'identità attuale,
/// farebbero bloccare l'avvio dal controllo delle firme.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn stale_binaries_cmd(src_dir: &std::path::Path, install_dir: &std::path::Path, keep: &[&str]) -> String {
    use std::path::Path;
    let binaries = |dir: &Path| -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.file_type().map_or(false, |t| t.is_file()))
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| {
                        let n = n.to_lowercase();
                        n.ends_with(".exe") || n.ends_with(".dll")
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let norm = |p: &Path| p.to_string_lossy().trim_end_matches('\\').to_lowercase();
    if norm(src_dir) == norm(install_dir) {
        return String::new();
    }
    let shipped: Vec<String> = binaries(src_dir).iter().map(|n| n.to_lowercase()).collect();
    if shipped.is_empty() {
        return String::new();
    }
    binaries(install_dir)
        .into_iter()
        .filter(|n| {
            let lower = n.to_lowercase();
            !shipped.contains(&lower) && !keep.iter().any(|k| k.eq_ignore_ascii_case(n))
        })
        .map(|n| format!("del /F /Q \"{}\"\n", install_dir.join(n).display()))
        .collect()
}

/// Windows: `path` è firmato da Clanto, con l'EKU attuale o con il nome della
/// società (versioni firmate prima dell'EKU). Vale anche senza EKU al build.
#[cfg(windows)]
pub fn signed_by_clanto(path: &Path) -> bool {
    clanto_win::trust::embedded_signer(path).map_or(false, |signer| {
        signing_eku().map_or(false, |eku| signer.has_eku(eku)) || signer.name.starts_with("Clanto Services")
    })
}

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn tampered_text(files: &str) -> String {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let text = crate::lang::translate("clanto_tampered_error".to_owned());
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let text = crate::lang::translate_locale("clanto_tampered_error".to_owned(), "");
    format!("{text}\n\n{files}")
}

#[cfg(any(windows, target_os = "macos"))]
fn report_tampered(files: &str) {
    log::error!("tampered ClantoDesk binaries: {files}");
    crate::clanto::audit_sink::emit("clanto_binary_signature", &serde_json::json!({ "files": files }));
}

/// Windows: `librustdesk.dll` verifica l'eseguibile che l'ha caricata.
/// `false`: il processo deve terminare (errore già mostrato se c'è un utente).
#[cfg(windows)]
pub fn verify_own_exe() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return true;
    };
    let Err(err) = verify_windows_file(&exe) else {
        return true;
    };
    log::error!("{err}");
    let name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    report_tampered(&name);
    let interactive = std::env::var_os("CI").is_none()
        && crate::platform::get_current_process_session_id().map_or(false, |s| s != 0);
    if interactive {
        use std::os::windows::ffi::OsStrExt;
        let wide = |s: &str| std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect::<Vec<u16>>();
        let text = wide(&tampered_text(&name));
        let title = wide(&crate::lang::translate("Tampered executable".to_owned()));
        unsafe {
            winapi::um::winuser::MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                winapi::um::winuser::MB_OK | winapi::um::winuser::MB_ICONERROR | winapi::um::winuser::MB_TOPMOST,
            );
        }
    }
    false
}

#[cfg(target_os = "macos")]
mod mac {
    use core_foundation::{
        base::{CFRelease, TCFType},
        string::CFString,
        url::CFURL,
    };
    use std::{ffi::c_void, path::Path};

    type SecStaticCodeRef = *const c_void;
    type SecRequirementRef = *const c_void;
    const CHECK_ALL_ARCHITECTURES: u32 = 1 << 0;
    const CHECK_NESTED_CODE: u32 = 1 << 3;
    const STRICT_VALIDATE: u32 = 1 << 4;

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        fn SecStaticCodeCreateWithPath(path: *const c_void, flags: u32, code: *mut SecStaticCodeRef) -> i32;
        fn SecRequirementCreateWithString(text: *const c_void, flags: u32, requirement: *mut SecRequirementRef) -> i32;
        fn SecStaticCodeCheckValidity(code: SecStaticCodeRef, flags: u32, requirement: SecRequirementRef) -> i32;
    }

    /// Bundle integro (codice annidato compreso) e firmato Developer ID dal team.
    pub fn check_bundle(bundle: &Path, team: &str) -> Result<(), String> {
        let url = CFURL::from_path(bundle, true).ok_or("invalid bundle path")?;
        let requirement = CFString::new(&format!(
            "anchor apple generic and certificate leaf[subject.OU] = \"{team}\""
        ));
        unsafe {
            let mut code: SecStaticCodeRef = std::ptr::null();
            let status = SecStaticCodeCreateWithPath(url.as_concrete_TypeRef() as *const c_void, 0, &mut code);
            if status != 0 || code.is_null() {
                return Err(format!("code object error {status}"));
            }
            let mut req: SecRequirementRef = std::ptr::null();
            let status = SecRequirementCreateWithString(requirement.as_concrete_TypeRef() as *const c_void, 0, &mut req);
            if status != 0 || req.is_null() {
                CFRelease(code);
                return Err(format!("requirement error {status}"));
            }
            let status = SecStaticCodeCheckValidity(code, CHECK_ALL_ARCHITECTURES | CHECK_NESTED_CODE | STRICT_VALIDATE, req);
            CFRelease(req);
            CFRelease(code);
            if status != 0 {
                return Err(format!("signature not valid for team {team} (OSStatus {status})"));
            }
        }
        Ok(())
    }
}

/// macOS: bundle `.app` integro e firmato dal team Clanto.
#[cfg(target_os = "macos")]
pub fn verify_macos_app(app: &Path) -> ResultType<()> {
    let Some(team) = signing_team() else {
        return Ok(());
    };
    mac::check_bundle(app, team).map_err(|err| hbb_common::anyhow::anyhow!("{}: {err}", app.display()))
}

/// macOS: il bundle da cui gira questo processo, se è un `.app`.
#[cfg(target_os = "macos")]
fn own_bundle() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|p| p.extension().map_or(false, |e| e == "app"))
        .map(Path::to_path_buf)
}

/// macOS: verifica del proprio bundle; `Err` con il messaggio da mostrare.
#[cfg(target_os = "macos")]
pub fn verify_own_bundle() -> Result<(), String> {
    let Some(bundle) = own_bundle() else {
        return Ok(());
    };
    match verify_macos_app(&bundle) {
        Ok(()) => Ok(()),
        Err(err) => {
            log::error!("{err}");
            let name = bundle.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            report_tampered(&name);
            Err(tampered_text(&name))
        }
    }
}

/// Messaggio di eseguibile manomesso per l'interfaccia, vuoto se integro.
/// Su Windows lo mostrano già l'exe e `verify_own_exe` prima dell'interfaccia.
pub fn tamper_message() -> String {
    #[cfg(target_os = "macos")]
    if let Err(message) = verify_own_bundle() {
        return message;
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_binaries_are_only_the_ones_no_longer_shipped() {
        let base = std::env::temp_dir().join(format!("clanto-stale-{}", std::process::id()));
        let (src, dst) = (base.join("src"), base.join("dst"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();
        for f in ["clantodesk.exe", "librustdesk.dll"] {
            std::fs::write(src.join(f), b"").unwrap();
        }
        for f in ["clantodesk.exe", "LIBRUSTDESK.dll", "file_selector_windows_plugin.dll", "RuntimeBroker_rustdesk.exe", "notes.txt"] {
            std::fs::write(dst.join(f), b"").unwrap();
        }
        let cmd = stale_binaries_cmd(&src, &dst, &["RuntimeBroker_rustdesk.exe"]);
        let _ = std::fs::remove_dir_all(&base);
        assert_eq!(cmd.lines().count(), 1, "{cmd}");
        assert!(cmd.starts_with("del /F /Q \"") && cmd.contains("file_selector_windows_plugin.dll"));
        assert!(stale_binaries_cmd(&src, &src, &[]).is_empty());
    }

    #[test]
    fn tampered_text_lists_the_files() {
        assert!(tampered_text("librustdesk.dll").ends_with("\n\nlibrustdesk.dll"));
    }
}
