// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Firma del codice. Gli aggiornamenti automatici si installano solo se
//! firmati da Clanto (Authenticode su Windows, Developer ID del team su
//! macOS). Su Windows, con ClantoDesk installato, servizio e interfaccia
//! verificano all'avvio ogni `.exe` e `.dll` della cartella d'installazione:
//! un binario non firmato da Clanto ferma il servizio (niente accesso remoto)
//! e l'interfaccia mostra «eseguibile manomesso» e si chiude.
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

/// Firmatari ammessi per i binari nella cartella d'installazione: Clanto e i
/// componenti Microsoft ridistribuiti (runtime, RuntimeBroker).
#[cfg(windows)]
const MICROSOFT_SIGNERS: &[&str] = &["Microsoft Windows", "Microsoft Corporation"];

/// Esito, per ogni file, di `Get-AuthenticodeSignature`: (nome, stato, soggetto).
#[cfg(windows)]
fn windows_signatures(files: &[std::path::PathBuf]) -> ResultType<Vec<(String, String, String)>> {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    let powershell = Path::new(&system_root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let list = files.iter().map(|f| f.to_string_lossy()).collect::<Vec<_>>().join("|");
    let output = Command::new(powershell)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "foreach ($f in $env:CLANTO_SIGNED_FILES.Split('|')) { $s = Get-AuthenticodeSignature -LiteralPath $f; Write-Output ((Split-Path $f -Leaf) + [char]9 + $s.Status + [char]9 + $s.SignerCertificate.Subject) }",
        ])
        .env("CLANTO_SIGNED_FILES", list)
        .output()?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim_end().splitn(3, '\t');
            Some((parts.next()?.to_owned(), parts.next()?.to_owned(), parts.next().unwrap_or_default().to_owned()))
        })
        .collect())
}

/// File non firmati da Clanto (o da Microsoft) fra quelli verificati; ogni
/// file senza esito conta come manomesso.
#[cfg_attr(not(windows), allow(dead_code))]
fn tampered_files(names: &[String], results: &[(String, String, String)], signers: &[&str]) -> Vec<String> {
    names
        .iter()
        .filter(|name| {
            !results.iter().any(|(file, status, subject)| {
                file.eq_ignore_ascii_case(name) && signers.iter().any(|s| signer_matches(status, subject, s))
            })
        })
        .cloned()
        .collect()
}

/// Windows, ClantoDesk installato: ogni `.exe` e `.dll` della cartella
/// d'installazione deve essere firmato da Clanto (o Microsoft). `Err` con i
/// file manomessi: chi chiama non deve proseguire.
#[cfg(windows)]
pub fn verify_install_dir() -> Result<(), Vec<String>> {
    if !enforced() || !crate::platform::is_installed() {
        return Ok(());
    }
    let (_, install_dir, _, _) = crate::platform::windows::get_install_info();
    let files: Vec<std::path::PathBuf> = std::fs::read_dir(&install_dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.extension()
                            .map_or(false, |e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("dll"))
                })
                .collect()
        })
        .unwrap_or_default();
    if files.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = files
        .iter()
        .filter_map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    let results = match windows_signatures(&files) {
        Ok(results) => results,
        Err(err) => {
            // Senza PowerShell la verifica non si fa: non si blocca l'uso.
            hbb_common::log::error!("signature check unavailable: {err}");
            return Ok(());
        }
    };
    let mut signers = vec![WINDOWS_SIGNER];
    signers.extend_from_slice(MICROSOFT_SIGNERS);
    let tampered = tampered_files(&names, &results, &signers);
    if tampered.is_empty() {
        return Ok(());
    }
    hbb_common::log::error!("tampered ClantoDesk binaries: {}", tampered.join(", "));
    crate::clanto::audit_sink::emit(
        "clanto_binary_signature",
        &serde_json::json!({ "files": tampered }),
    );
    Err(tampered)
}

/// Messaggio per l'interfaccia, vuoto se i binari sono integri. Chiamato
/// dall'interfaccia all'avvio (Flutter, in un thread a parte).
pub fn tamper_message() -> String {
    #[cfg(windows)]
    if let Err(files) = verify_install_dir() {
        return format!(
            "{}\n\n{}",
            crate::lang::translate("clanto_tampered_error".to_owned()),
            files.join("\n")
        );
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsigned_or_missing_results_are_tampered() {
        let names = vec!["clantodesk.exe".to_owned(), "librustdesk.dll".to_owned(), "x.dll".to_owned(), "vc.dll".to_owned()];
        let clanto = "CN=Clanto Services srl, O=Clanto Services srl, C=IT";
        let results = vec![
            ("ClantoDesk.exe".to_owned(), "Valid".to_owned(), clanto.to_owned()),
            ("librustdesk.dll".to_owned(), "HashMismatch".to_owned(), clanto.to_owned()),
            ("vc.dll".to_owned(), "Valid".to_owned(), "CN=Microsoft Corporation, O=Microsoft Corporation".to_owned()),
        ];
        let tampered = tampered_files(&names, &results, &["Clanto Services srl", "Microsoft Corporation"]);
        assert_eq!(tampered, vec!["librustdesk.dll".to_owned(), "x.dll".to_owned()]);
    }

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
