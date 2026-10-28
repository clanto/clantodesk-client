// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Linux: aggiornamento dal servizio root (CLANTO.md, «Aggiornamenti»).
//!
//! Vale per le installazioni da pacchetto .deb o .rpm (non AppImage o Flatpak).
//! Il servizio scarica dalla release ClantoDesk indicata da `clantodesk.php` il
//! pacchetto della stessa famiglia e architettura, in una cartella di root,
//! controlla nome e versione del pacchetto e lo installa con il gestore del
//! sistema in un'unità systemd separata: lo script del pacchetto ferma il
//! servizio, che non deve portarsi dietro l'installazione.

use crate::updater::{DISABLED_RECHECK, DUR_ONE_DAY, INITIAL_CHECK_DELAY, RETRY_INTERVAL};
use hbb_common::{bail, config::Config, log, ResultType};
use std::{
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

const PACKAGE: &str = "clantodesk";

static RUNNING: AtomicBool = AtomicBool::new(false);

struct RunningGuard;

impl Drop for RunningGuard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Deb,
    Rpm,
    RpmSuse,
}

fn succeeds(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn is_suse() -> bool {
    std::fs::read_to_string("/etc/os-release")
        .map(|s| {
            s.lines()
                .filter(|l| l.starts_with("ID=") || l.starts_with("ID_LIKE="))
                .any(|l| l.to_ascii_lowercase().contains("suse"))
        })
        .unwrap_or(false)
}

fn installed_kind() -> Option<Kind> {
    if succeeds("dpkg-query", &["-W", "-f=${Status}", PACKAGE])
        .is_some_and(|s| s.trim() == "install ok installed")
    {
        return Some(Kind::Deb);
    }
    succeeds("rpm", &["-q", PACKAGE])?;
    Some(if is_suse() { Kind::RpmSuse } else { Kind::Rpm })
}

fn asset_name(kind: Kind, version: &str, arch: &str) -> String {
    match kind {
        Kind::Deb => format!("{PACKAGE}-{version}-{arch}.deb"),
        Kind::Rpm => format!("{PACKAGE}-{version}-{arch}.rpm"),
        Kind::RpmSuse => format!("{PACKAGE}-{version}-{arch}-suse.rpm"),
    }
}

/// Nome e versione dichiarati dal pacchetto scaricato.
fn package_identity(kind: Kind, file: &Path) -> Option<(String, String)> {
    let file = file.to_str()?;
    match kind {
        Kind::Deb => {
            let out = succeeds("dpkg-deb", &["-f", file, "Package", "Version"])?;
            let field = |name: &str| {
                out.lines()
                    .find_map(|l| l.strip_prefix(name))
                    .map(|v| v.trim().to_owned())
            };
            Some((field("Package:")?, field("Version:")?))
        }
        Kind::Rpm | Kind::RpmSuse => {
            let out = succeeds(
                "rpm",
                &["-qp", "--nosignature", "--qf", "%{NAME} %{VERSION}", file],
            )?;
            let (name, version) = out.trim().split_once(' ')?;
            Some((name.to_owned(), version.to_owned()))
        }
    }
}

/// Script dell'unità di installazione: `$1` è il pacchetto. Il riavvio finale
/// serve all'aggiornamento .rpm, il cui %post avvia il servizio solo se fermo.
fn install_script(kind: Kind) -> &'static str {
    match kind {
        Kind::Deb => {
            r#"f="$1"
export DEBIAN_FRONTEND=noninteractive
if command -v apt-get >/dev/null 2>&1; then apt-get install -y "$f"; else dpkg -i "$f"; fi
rc=$?
rm -f "$f"
[ $rc -eq 0 ] && systemctl restart clantodesk
exit $rc"#
        }
        Kind::Rpm | Kind::RpmSuse => {
            r#"f="$1"
if command -v dnf >/dev/null 2>&1; then dnf install -y "$f"
elif command -v zypper >/dev/null 2>&1; then zypper --non-interactive install --allow-unsigned-rpm "$f"
elif command -v yum >/dev/null 2>&1; then yum install -y "$f"
else rpm -Uvh "$f"; fi
rc=$?
rm -f "$f"
[ $rc -eq 0 ] && systemctl restart clantodesk
exit $rc"#
        }
    }
}

/// Cartella di root per i download; le versioni precedenti si cancellano.
fn download_dir() -> ResultType<PathBuf> {
    let dir = Config::path("update");
    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    for entry in std::fs::read_dir(&dir)?.flatten() {
        let _ = std::fs::remove_file(entry.path());
    }
    Ok(dir)
}

/// Nessuna sessione attiva nei `--server` (utente attivo e schermata di accesso).
/// Un `--server` che non risponde non ha sessioni; una risposta diversa da «nessuna» blocca.
pub fn no_active_sessions() -> bool {
    use crate::ipc::Data;
    let mut uids = vec![0_u32];
    if let Ok(uid) = crate::platform::linux::get_active_userid().parse::<u32>() {
        if uid != 0 {
            uids.push(uid);
        }
    }
    let Ok(runtime) = hbb_common::tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return false;
    };
    runtime.block_on(async {
        for uid in uids {
            let Ok(mut conn) = crate::ipc::connect_for_uid(1000, uid, "").await else {
                continue;
            };
            if conn.send(&Data::HasNoActiveConns(None)).await.is_err() {
                return false;
            }
            match conn.next_timeout(1000).await {
                Ok(Some(Data::HasNoActiveConns(Some(true)))) => {}
                _ => return false,
            }
        }
        true
    })
}

/// Controlla e, se c'è una versione più recente, avvia l'installazione.
/// `manual`: richiesta dell'utente, senza controllo dell'opzione e delle sessioni.
/// `Ok(true)`: installazione avviata.
pub fn check_and_install(manual: bool) -> ResultType<bool> {
    use super::service_update::{set_state, DOWNLOADING, INSTALLING};
    if RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        bail!("another update is already running");
    }
    let _guard = RunningGuard;
    if !crate::platform::is_root() || !crate::platform::is_installed() {
        bail!("the update needs the installed system service");
    }
    if !manual && !Config::get_bool_option(base::config::keys::OPTION_ALLOW_AUTO_UPDATE) {
        return Ok(false);
    }
    let Some(kind) = installed_kind() else {
        bail!("ClantoDesk is not installed from a .deb or .rpm package");
    };
    let arch = match std::env::consts::ARCH {
        arch @ ("x86_64" | "aarch64") => arch,
        other => bail!("unsupported architecture: {other}"),
    };
    crate::common::do_check_software_update()?;
    let update_url = crate::common::SOFTWARE_UPDATE_URL.lock().unwrap().clone();
    if update_url.is_empty() {
        return Ok(false);
    }
    let base_url = update_url.replace("tag", "download");
    let version = base_url.rsplit('/').next().unwrap_or_default().to_owned();
    let url = format!("{base_url}/{}", asset_name(kind, &version, arch));
    let Some(file_name) = crate::updater::get_update_download_file_from_url(&url)
        .and_then(|p| p.file_name().map(|n| n.to_owned()))
    else {
        bail!("update URL not allowed: {url}");
    };
    if manual {
        set_state(DOWNLOADING);
    }
    let file = download_dir()?.join(file_name);
    let client = crate::hbbs_http::create_http_client_with_url_strict(&url)?;
    let response = client.get(&url).send()?;
    if !response.status().is_success() {
        bail!("download failed: {}", response.status());
    }
    let data = response.bytes()?;
    std::fs::File::create(&file)?.write_all(&data)?;

    match package_identity(kind, &file) {
        Some((name, v)) if name == PACKAGE && v == version => {}
        other => {
            let _ = std::fs::remove_file(&file);
            bail!("unexpected package {other:?}, expected {PACKAGE} {version}");
        }
    }
    if !manual && !no_active_sessions() {
        let _ = std::fs::remove_file(&file);
        bail!("active session, update deferred");
    }
    if manual {
        set_state(INSTALLING);
    }
    let unit = format!(
        "{PACKAGE}-update-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default()
    );
    let Some(file_arg) = file.to_str() else {
        bail!("invalid download path");
    };
    let status = Command::new("systemd-run")
        .args(["--unit", &unit, "--collect", "--quiet", "/bin/sh", "-c"])
        .arg(install_script(kind))
        .args(["sh", file_arg])
        .status();
    match status {
        Ok(s) if s.success() => {
            log::info!("ClantoDesk {version} installation started ({unit})");
            Ok(true)
        }
        Ok(s) => {
            let _ = std::fs::remove_file(&file);
            bail!("systemd-run failed: {s}")
        }
        Err(e) => {
            let _ = std::fs::remove_file(&file);
            bail!("systemd-run: {e}")
        }
    }
}

/// Servizio root: aggiornamento automatico con `allow-auto-update` (opzione o
/// policy `AutoUpdate`), 30 secondi dopo l'avvio e poi ogni giorno, solo senza
/// sessioni attive.
pub fn start_auto_update() {
    let spawned = std::thread::Builder::new()
        .name("clanto-auto-update".to_owned())
        .spawn(|| {
            std::thread::sleep(INITIAL_CHECK_DELAY);
            loop {
                let interval = if !Config::get_bool_option(
                    base::config::keys::OPTION_ALLOW_AUTO_UPDATE,
                ) {
                    DISABLED_RECHECK
                } else if !no_active_sessions() {
                    RETRY_INTERVAL
                } else {
                    match check_and_install(false) {
                        Ok(true) => RETRY_INTERVAL,
                        Ok(false) => DUR_ONE_DAY,
                        Err(e) => {
                            log::warn!("auto update: {e}");
                            RETRY_INTERVAL
                        }
                    }
                };
                std::thread::sleep(interval);
            }
        });
    if let Err(e) = spawned {
        log::error!("auto update thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_names_match_the_release_workflow() {
        assert_eq!(
            asset_name(Kind::Deb, "1.5.1", "x86_64"),
            "clantodesk-1.5.1-x86_64.deb"
        );
        assert_eq!(
            asset_name(Kind::Rpm, "1.5.1", "aarch64"),
            "clantodesk-1.5.1-aarch64.rpm"
        );
        assert_eq!(
            asset_name(Kind::RpmSuse, "1.5.1", "x86_64"),
            "clantodesk-1.5.1-x86_64-suse.rpm"
        );
    }

    #[test]
    fn install_scripts_take_the_package_as_argument() {
        for kind in [Kind::Deb, Kind::Rpm, Kind::RpmSuse] {
            let script = install_script(kind);
            assert!(script.starts_with("f=\"$1\""));
            assert!(script.contains("systemctl restart clantodesk"));
        }
    }
}
