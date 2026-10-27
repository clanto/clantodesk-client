// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Disinstallazione Windows (CLANTO.md, «Disinstallazione»).
//!
//! `--uninstall` chiede se mantenere la configurazione (ID, password, rubrica,
//! impostazioni) o cancellare tutto: configurazione di ogni utente e del
//! servizio, log, report, quarantena, dati di Flutter e chiavi di registro.
//! Il programma (exe, dll, servizio, collegamenti) lo toglie sempre
//! `get_uninstall()` di upstream; qui si aggiungono solo i comandi in più.
//!
//! Fino alla 1.3 ClantoDesk si chiamava «RustDesk» in tutto. Un'installazione
//! «RustDesk» firmata da Clanto, o configurata con un server Clanto, si toglie
//! in automatico; un RustDesk ufficiale solo se l'utente lo conferma.
//!
//! Senza finestra: `--keep-config`, `--purge`, `--remove-rustdesk`.
//! MSI: `msiexec /x … CLANTO_PURGE=1 [CLANTO_REMOVE_RUSTDESK=1]`, che esegue
//! `--clanto-purge-data` prima di togliere i file.

#![cfg_attr(not(windows), allow(dead_code))]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    KeepConfig,
    Purge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RustDesk {
    Absent,
    /// Vecchia ClantoDesk con il nome «RustDesk».
    Clanto,
    Official,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub mode: Mode,
    pub remove_rustdesk: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Flags {
    mode: Option<Mode>,
    remove_rustdesk: bool,
}

fn parse_flags(args: &[String]) -> Flags {
    let mut flags = Flags::default();
    for arg in args {
        match arg.as_str() {
            "--keep-config" => flags.mode = Some(Mode::KeepConfig),
            "--purge" => flags.mode = Some(Mode::Purge),
            "--remove-rustdesk" => flags.remove_rustdesk = true,
            _ => {}
        }
    }
    flags
}

/// Scelta senza finestra: una vecchia ClantoDesk «RustDesk» si toglie sempre,
/// un RustDesk ufficiale solo con `--remove-rustdesk`.
fn unattended(flags: Flags, rustdesk: RustDesk) -> Choice {
    Choice {
        mode: flags.mode.unwrap_or(Mode::KeepConfig),
        remove_rustdesk: match rustdesk {
            RustDesk::Absent => false,
            RustDesk::Clanto => true,
            RustDesk::Official => flags.remove_rustdesk,
        },
    }
}

/// La configurazione nomina uno dei server Clanto fissati al build.
pub fn mentions_clanto_server(text: &str, build_servers: &str) -> bool {
    let text = text.to_lowercase();
    build_servers
        .split(',')
        .map(|s| crate::clanto::config::host_of(s).to_lowercase())
        .any(|host| !host.is_empty() && text.contains(&host))
}

/// Percorso letterale in un file batch: `%` raddoppiato, niente virgolette.
fn batch_path(path: &str) -> Option<String> {
    (!path.contains('"') && !path.is_empty()).then(|| path.replace('%', "%%"))
}

const QUIET: &str = ">nul 2>&1";
/// Cartelle in cui Flutter (path_provider) salva i dati: CompanyName di
/// `Runner.rc` attuale e delle versioni precedenti.
const FLUTTER_COMPANIES: &[&str] = &["Clanto Services srls", "Purslane Ltd", "com.carriez"];

/// Dati di `name` nei profili indicati (utenti e servizio) e chiavi di
/// registro non legate all'installazione. Le policy GPO
/// (`HKLM\Software\Policies\Clanto`) restano: le gestisce l'amministratore.
fn data_cmds(name: &str, profiles: &[String]) -> String {
    let mut out = String::new();
    for profile in profiles.iter().filter_map(|p| batch_path(p.trim_end_matches('\\'))) {
        for dir in [
            format!("{profile}\\AppData\\Roaming\\{name}"),
            format!("{profile}\\AppData\\Local\\{name}"),
        ] {
            out += &format!("if exist \"{dir}\" rd /s /q \"{dir}\" {QUIET}\n");
        }
        for company in FLUTTER_COMPANIES {
            let parent = format!("{profile}\\AppData\\Roaming\\{company}");
            out += &format!("if exist \"{parent}\\{name}\" rd /s /q \"{parent}\\{name}\" {QUIET}\n");
            // Solo se vuota: altri programmi possono usare la stessa cartella.
            out += &format!("if exist \"{parent}\" rd \"{parent}\" {QUIET}\n");
        }
    }
    for view in ["", " /reg:32"] {
        out += &format!("reg delete \"HKLM\\SOFTWARE\\{name}\" /f{view} {QUIET}\n");
    }
    out += &format!("reg delete \"HKLM\\SYSTEM\\CurrentControlSet\\Services\\EventLog\\Application\\{name}\" /f {QUIET}\n");
    out
}

/// Installazione «RustDesk» (vecchia ClantoDesk o RustDesk ufficiale): prima il
/// suo disinstallatore, che sa togliere anche MSI, stampante e driver, poi
/// quello che potrebbe essere rimasto.
fn rustdesk_program_cmds(install_dir: Option<&str>) -> String {
    let mut out = String::new();
    let dir = install_dir.and_then(|d| batch_path(d.trim_end_matches('\\')));
    if let Some(dir) = &dir {
        out += &format!("if exist \"{dir}\\rustdesk.exe\" \"{dir}\\rustdesk.exe\" --uninstall\n");
    }
    out += &format!(
        "sc stop RustDesk {QUIET}
sc delete RustDesk {QUIET}
taskkill /F /IM rustdesk.exe {QUIET}
reg delete \"HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\RustDesk\" /f {QUIET}
reg delete \"HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\RustDesk\" /f /reg:32 {QUIET}
reg delete HKEY_CLASSES_ROOT\\.rustdesk /f {QUIET}
reg delete HKEY_CLASSES_ROOT\\rustdesk /f {QUIET}
netsh advfirewall firewall delete rule name=\"RustDesk Service\" {QUIET}
if exist \"%ProgramData%\\Microsoft\\Windows\\Start Menu\\Programs\\RustDesk\" rd /s /q \"%ProgramData%\\Microsoft\\Windows\\Start Menu\\Programs\\RustDesk\" {QUIET}
if exist \"%PUBLIC%\\Desktop\\RustDesk.lnk\" del /f /q \"%PUBLIC%\\Desktop\\RustDesk.lnk\" {QUIET}
if exist \"%ProgramData%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\RustDesk Tray.lnk\" del /f /q \"%ProgramData%\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\RustDesk Tray.lnk\" {QUIET}
"
    );
    if let Some(dir) = &dir {
        out += &format!("if exist \"{dir}\" rd /s /q \"{dir}\" {QUIET}\n");
    }
    out
}

fn build_cmds(choice: Choice, rustdesk_dir: Option<&str>, profiles: &[String]) -> String {
    let mut out = String::new();
    if choice.remove_rustdesk {
        out += &rustdesk_program_cmds(rustdesk_dir);
    }
    if choice.mode == Mode::Purge {
        out += &data_cmds(&crate::get_app_name(), profiles);
        if choice.remove_rustdesk {
            out += &data_cmds("RustDesk", profiles);
        }
    }
    out
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::path::{Path, PathBuf};
    use winreg::{enums::*, RegKey};

    const UNINSTALL_KEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\RustDesk";

    /// Profili degli utenti (da ProfileList) e dei servizi.
    pub fn profiles() -> Vec<String> {
        let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned());
        let mut out = Vec::new();
        if let Ok(list) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ProfileList")
        {
            for sid in list.enum_keys().flatten().filter(|s| s.starts_with("S-1-5-21-")) {
                let path: Option<String> = list.open_subkey(&sid).and_then(|k| k.get_value("ProfileImagePath")).ok();
                if let Some(path) = path {
                    out.push(path.replace("%SystemDrive%", &system_drive).replace("%SystemRoot%", &system_root));
                }
            }
        }
        out.push(format!("{system_root}\\ServiceProfiles\\LocalService"));
        out.push(format!("{system_root}\\System32\\config\\systemprofile"));
        out
    }

    pub fn rustdesk_dir() -> Option<PathBuf> {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let from_registry = [KEY_WOW64_64KEY, KEY_WOW64_32KEY].into_iter().find_map(|view| {
            let key = hklm.open_subkey_with_flags(UNINSTALL_KEY, KEY_READ | view).ok()?;
            let dir: String = key.get_value("InstallLocation").ok()?;
            (!dir.trim().is_empty()).then(|| PathBuf::from(dir.trim()))
        });
        from_registry.or_else(|| {
            let dir = PathBuf::from(std::env::var("ProgramFiles").ok()?).join("RustDesk");
            dir.join("rustdesk.exe").exists().then_some(dir)
        })
    }

    fn config_mentions_clanto(profiles: &[String]) -> bool {
        let Some(servers) = option_env!("RENDEZVOUS_SERVER").filter(|_| crate::clanto::config::servers_fixed_at_build()) else {
            return false;
        };
        profiles.iter().any(|p| {
            ["RustDesk.toml", "RustDesk2.toml"].iter().any(|f| {
                let path = Path::new(p).join("AppData\\Roaming\\RustDesk\\config").join(f);
                std::fs::read_to_string(path).map_or(false, |text| mentions_clanto_server(&text, servers))
            })
        })
    }

    fn rustdesk_data_present(profiles: &[String]) -> bool {
        profiles.iter().any(|p| Path::new(p).join("AppData\\Roaming\\RustDesk").exists())
    }

    pub fn detect(profiles: &[String]) -> (RustDesk, Option<PathBuf>) {
        let dir = rustdesk_dir();
        let exe = dir.as_ref().map(|d| d.join("rustdesk.exe")).filter(|e| e.exists());
        if exe.is_none() && dir.as_ref().map_or(true, |d| !d.exists()) && !rustdesk_data_present(profiles) {
            return (RustDesk::Absent, None);
        }
        let clanto = exe.as_deref().map_or(false, crate::clanto::signature::signed_by_clanto)
            || config_mentions_clanto(profiles);
        (if clanto { RustDesk::Clanto } else { RustDesk::Official }, dir)
    }

    pub fn interactive() -> bool {
        std::env::var_os("CI").is_none()
            && crate::platform::get_current_process_session_id().map_or(false, |s| s != 0)
    }

    pub fn message_box(text: &str, flags: u32) -> i32 {
        use std::os::windows::ffi::OsStrExt;
        let wide = |s: &str| std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect::<Vec<u16>>();
        let text = wide(text);
        let title = wide(&crate::lang::translate("clanto_uninstall_title".to_owned()));
        unsafe {
            winapi::um::winuser::MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                flags | winapi::um::winuser::MB_TOPMOST | winapi::um::winuser::MB_SETFOREGROUND,
            )
        }
    }
}

/// `--uninstall [opzioni]`: chiede cosa fare (o legge le opzioni) e restituisce
/// i comandi da aggiungere alla disinstallazione; `None` = annullata.
#[cfg(windows)]
pub fn prepare(args: &[String]) -> Option<(Choice, String)> {
    use winapi::um::winuser::{IDNO, IDYES, MB_ICONQUESTION, MB_ICONWARNING, MB_YESNO, MB_YESNOCANCEL};
    let flags = parse_flags(args);
    let profiles = win::profiles();
    let (rustdesk, dir) = win::detect(&profiles);
    let choice = if flags.mode.is_some() || !win::interactive() {
        unattended(flags, rustdesk)
    } else {
        let mode = match win::message_box(&crate::lang::translate("clanto_uninstall_keep_config".to_owned()), MB_YESNOCANCEL | MB_ICONQUESTION) {
            IDYES => Mode::KeepConfig,
            IDNO => Mode::Purge,
            _ => return None,
        };
        let remove_rustdesk = match rustdesk {
            RustDesk::Absent => false,
            RustDesk::Clanto => true,
            RustDesk::Official => {
                win::message_box(&crate::lang::translate("clanto_uninstall_official_rustdesk".to_owned()), MB_YESNO | MB_ICONWARNING)
                    == IDYES
            }
        };
        Choice { mode, remove_rustdesk }
    };
    hbb_common::log::info!("uninstall: {choice:?}, RustDesk {rustdesk:?}");
    let dir = dir.map(|d| d.to_string_lossy().into_owned());
    Some((choice, build_cmds(choice, dir.as_deref(), &profiles)))
}

/// Dopo la disinstallazione: la cartella dell'utente corrente si cancella quando
/// questo processo, che tiene aperto il log, è uscito.
#[cfg(windows)]
pub fn finish(choice: Choice) {
    use std::os::windows::process::CommandExt;
    if choice.mode != Mode::Purge {
        return;
    }
    let Some(dir) = std::env::var("APPDATA").ok().and_then(|d| batch_path(&format!("{d}\\{}", crate::get_app_name()))) else {
        return;
    };
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("cmd")
        .raw_arg(format!("/D /C ping -n 4 127.0.0.1 >nul & rd /s /q \"{dir}\""))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

/// MSI con `CLANTO_PURGE=1`: azione differita, già amministratore, nessuna
/// finestra. Esegue subito la pulizia dei dati (e di RustDesk se richiesto).
#[cfg(windows)]
pub fn purge_now(args: &[String]) {
    let mut flags = parse_flags(args);
    flags.mode = Some(Mode::Purge);
    let profiles = win::profiles();
    let (rustdesk, dir) = win::detect(&profiles);
    let choice = unattended(flags, rustdesk);
    let dir = dir.map(|d| d.to_string_lossy().into_owned());
    let script = std::env::temp_dir().join(format!("clantodesk_purge_{}.bat", uuid::Uuid::new_v4().simple()));
    let body = format!("@echo off\nchcp 65001 >nul\n{}", build_cmds(choice, dir.as_deref(), &profiles));
    if let Err(err) = std::fs::write(&script, body) {
        hbb_common::log::error!("purge script: {err}");
        return;
    }
    let status = std::process::Command::new("cmd").arg("/D").arg("/C").arg(&script).status();
    hbb_common::log::info!("purge {choice:?}: {status:?}");
    let _ = std::fs::remove_file(&script);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn unattended_keeps_config_and_official_rustdesk_by_default() {
        let none = parse_flags(&[]);
        assert_eq!(unattended(none, RustDesk::Official), Choice { mode: Mode::KeepConfig, remove_rustdesk: false });
        assert_eq!(unattended(none, RustDesk::Clanto), Choice { mode: Mode::KeepConfig, remove_rustdesk: true });
        let all = parse_flags(&args(&["--purge", "--remove-rustdesk"]));
        assert_eq!(unattended(all, RustDesk::Official), Choice { mode: Mode::Purge, remove_rustdesk: true });
        assert!(!unattended(all, RustDesk::Absent).remove_rustdesk);
    }

    #[test]
    fn clanto_server_found_in_old_config() {
        let config = "rendezvous_server = 'rd.example.test:21116'\n";
        assert!(mentions_clanto_server(config, "RD.example.test:21116,relay.example.test"));
        assert!(!mentions_clanto_server(config, "other.example.test"));
        assert!(!mentions_clanto_server(config, ""));
    }

    #[test]
    fn purge_removes_data_of_every_profile_but_not_policies() {
        let profiles = vec!["C:\\Users\\Utente 100%".to_owned(), "C:\\Windows\\ServiceProfiles\\LocalService".to_owned()];
        let cmds = data_cmds("ClantoDesk", &profiles);
        assert!(cmds.contains("rd /s /q \"C:\\Users\\Utente 100%%\\AppData\\Roaming\\ClantoDesk\""));
        assert!(cmds.contains("C:\\Windows\\ServiceProfiles\\LocalService\\AppData\\Roaming\\ClantoDesk"));
        assert!(cmds.contains("Clanto Services srls\\ClantoDesk"));
        assert!(cmds.contains("EventLog\\Application\\ClantoDesk"));
        assert!(!cmds.contains("Policies"));
    }

    #[test]
    fn keep_config_without_rustdesk_adds_nothing() {
        let choice = Choice { mode: Mode::KeepConfig, remove_rustdesk: false };
        assert!(build_cmds(choice, Some("C:\\Program Files\\RustDesk"), &["C:\\Users\\a".to_owned()]).is_empty());
        let rustdesk = build_cmds(Choice { remove_rustdesk: true, ..choice }, Some("C:\\Program Files\\RustDesk\\"), &[]);
        assert!(rustdesk.starts_with("if exist \"C:\\Program Files\\RustDesk\\rustdesk.exe\" \"C:\\Program Files\\RustDesk\\rustdesk.exe\" --uninstall"));
        assert!(rustdesk.contains("sc delete RustDesk") && !rustdesk.contains("AppData"));
    }
}
