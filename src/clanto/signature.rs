// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Firma del codice. Gli aggiornamenti automatici si installano solo se
//! firmati da Clanto (Authenticode su Windows, Developer ID del team su
//! macOS); il servizio Windows controlla all'avvio i propri binari e, se la
//! firma non torna, registra un allarme nell'audit.
//!
//! Non ferma chi è già amministratore della macchina (può togliere anche il
//! controllo): per quello servono le regole di App Control/AppLocker di
//! Windows sul firmatario.

#[cfg(any(windows, target_os = "macos"))]
use hbb_common::{bail, ResultType};
#[cfg(any(windows, target_os = "macos"))]
use std::{path::Path, process::Command};

/// CN del certificato Authenticode (Azure Trusted Signing).
#[cfg(windows)]
const WINDOWS_SIGNER: &str = match option_env!("CLANTO_WINDOWS_SIGNER") {
    Some(signer) => signer,
    None => "Clanto Services srl",
};
/// Team ID Apple del certificato Developer ID.
#[cfg(target_os = "macos")]
const MAC_TEAM_ID: &str = "DCP3L7L7BW";

/// Controlli attivi solo sui binari di release (server fissati al build): le
/// build di sviluppo non sono firmate.
#[cfg(any(windows, target_os = "macos"))]
fn enforced() -> bool {
    crate::clanto::config::servers_fixed_at_build()
}

#[cfg(windows)]
fn windows_signature(path: &Path) -> ResultType<(String, String)> {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    let powershell = Path::new(&system_root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    // Percorso via variabile d'ambiente: nessun problema di quoting.
    let output = Command::new(powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$s = Get-AuthenticodeSignature -LiteralPath $env:CLANTO_SIGNED_PATH; Write-Output $s.Status; Write-Output $s.SignerCertificate.Subject",
        ])
        .env("CLANTO_SIGNED_PATH", path)
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines().map(str::trim);
    Ok((
        lines.next().unwrap_or_default().to_owned(),
        lines.next().unwrap_or_default().to_owned(),
    ))
}

#[cfg_attr(not(windows), allow(dead_code))]
fn signer_matches(status: &str, subject: &str, expected: &str) -> bool {
    // Il CN deve coincidere: "contiene" accetterebbe "Clanto Services srl Fake".
    let common_name = subject
        .split(", ")
        .find_map(|part| part.trim().strip_prefix("CN="))
        .unwrap_or_default();
    status == "Valid" && common_name.eq_ignore_ascii_case(expected)
}

/// Windows: `path` ha una firma Authenticode valida di Clanto.
#[cfg(windows)]
pub fn verify_windows_file(path: &Path) -> ResultType<()> {
    if !enforced() {
        return Ok(());
    }
    let (status, subject) = windows_signature(path)?;
    if !signer_matches(&status, &subject, WINDOWS_SIGNER) {
        bail!(
            "{} is not signed by Clanto (status {status:?}, signer {subject:?})",
            path.display()
        );
    }
    Ok(())
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn team_matches(codesign_details: &str, team: &str) -> bool {
    codesign_details
        .lines()
        .any(|line| line.trim() == format!("TeamIdentifier={team}"))
}

/// macOS: bundle `.app` firmato integro e dal team Clanto.
#[cfg(target_os = "macos")]
pub fn verify_macos_app(app: &Path) -> ResultType<()> {
    if !enforced() {
        return Ok(());
    }
    let verify = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .output()?;
    if !verify.status.success() {
        bail!(
            "{} signature is not valid: {}",
            app.display(),
            String::from_utf8_lossy(&verify.stderr).trim()
        );
    }
    // `codesign -dv` scrive i dettagli su stderr.
    let details = Command::new("/usr/bin/codesign").args(["-dv", "--verbose=2"]).arg(app).output()?;
    if !team_matches(&String::from_utf8_lossy(&details.stderr), MAC_TEAM_ID) {
        bail!("{} is not signed by the Clanto team {MAC_TEAM_ID}", app.display());
    }
    Ok(())
}

/// Servizio Windows: controllo dei binari installati, in un thread; esito nel
/// log e, se negativo, nell'audit (`clanto_binary_signature`).
#[cfg(windows)]
pub fn check_installed_binaries() {
    if !enforced() || !crate::platform::is_installed() {
        return;
    }
    std::thread::spawn(|| {
        let (_, install_dir, _, _) = crate::platform::windows::get_install_info();
        let mut files = vec![];
        if let Ok(exe) = std::env::current_exe() {
            files.push(exe);
        }
        files.push(Path::new(&install_dir).join(clanto_vcam::DLL_NAME));
        for file in files.into_iter().filter(|f| f.is_file()) {
            match verify_windows_file(&file) {
                Ok(()) => hbb_common::log::info!("signature ok: {}", file.display()),
                Err(err) => {
                    hbb_common::log::error!("{err}");
                    crate::clanto::audit_sink::emit(
                        "clanto_binary_signature",
                        &serde_json::json!({
                            "file": file.file_name().map(|n| n.to_string_lossy().into_owned()),
                            "error": err.to_string(),
                        }),
                    );
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signer_and_team_checks() {
        let subject = "CN=Clanto Services srl, O=Clanto Services srl, L=Melito di Napoli, S=Napoli, C=IT, PostalCode=80017";
        assert!(signer_matches("Valid", subject, "Clanto Services srl"));
        assert!(!signer_matches("HashMismatch", subject, "Clanto Services srl"));
        assert!(!signer_matches("Valid", "CN=Clanto Services srl Fake, O=x", "Clanto Services srl"));
        assert!(!signer_matches("Valid", "CN=Other, O=Clanto Services srl", "Clanto Services srl"));
        assert!(team_matches("Identifier=x\nTeamIdentifier=DCP3L7L7BW\n", "DCP3L7L7BW"));
        assert!(!team_matches("TeamIdentifier=ABCDEF1234\n", "DCP3L7L7BW"));
        assert!(!team_matches("TeamIdentifier=not set\n", "DCP3L7L7BW"));
    }
}
