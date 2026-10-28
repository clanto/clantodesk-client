// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Brand dei partner, livello 1 (CLANTO.md, «Build per i partner»).
//!
//! Cambia solo ciò che l'utente vede: nome, azienda, link, nomi dei file della
//! release e repo degli aggiornamenti. I valori arrivano al build da
//! `.github/scripts/apply-brand.py`; senza, resta ClantoDesk. `APP_NAME`,
//! servizio, cartelle, bundle id e firma restano quelli di ClantoDesk.

const fn or(value: Option<&'static str>, default: &'static str) -> &'static str {
    match value {
        Some(v) if !v.is_empty() => v,
        _ => default,
    }
}

pub const BRANDED: bool = matches!(option_env!("CLANTO_BRAND_NAME"), Some(v) if !v.is_empty());
/// Nome visualizzato.
pub const NAME: &str = or(option_env!("CLANTO_BRAND_NAME"), "ClantoDesk");
/// Azienda che compare al posto di «Clanto» nei testi.
pub const COMPANY: &str = or(option_env!("CLANTO_BRAND_COMPANY"), "Clanto");
/// Inizio dei nomi dei file della release.
pub const FILE_PREFIX: &str = or(option_env!("CLANTO_BRAND_FILE_PREFIX"), "clantodesk");
/// Repo `github.com/clanto/<repo>` delle release e degli aggiornamenti.
pub const UPDATE_REPO: &str = or(option_env!("CLANTO_BRAND_REPO"), "clantodesk-client");
pub const SUPPORT_URL: &str = or(
    option_env!("CLANTO_BRAND_SUPPORT_URL"),
    "https://supporto.clanto.it/clantodesk",
);
pub const WEBSITE_URL: &str = or(option_env!("CLANTO_BRAND_WEBSITE_URL"), "https://clanto.it");

const UPDATE_API: &str = "https://clanto.it/api/clantodesk.php";

/// Nomi di componenti di sistema, che al livello 1 restano ClantoDesk.
const SYSTEM_NAMES: &[&str] = &["ClantoDesk Camera", "ClantoDesk Input"];
/// Testi in cui «Clanto» è chi firma i binari, che resta Clanto.
const SIGNER_KEYS: &[&str] = &["clanto_tampered_error"];

/// Indirizzo del controllo aggiornamenti: con il brand chiede la release del
/// repo del partner, con lo stesso tag di ClantoDesk.
pub fn update_api_url() -> String {
    if BRANDED {
        format!("{UPDATE_API}?repo={UPDATE_REPO}")
    } else {
        UPDATE_API.to_owned()
    }
}

/// Valori per Flutter (`main_get_common("clanto-brand")`).
pub fn to_json() -> String {
    serde_json::json!({
        "branded": BRANDED,
        "name": NAME,
        "company": COMPANY,
        "support-url": SUPPORT_URL,
        "website-url": WEBSITE_URL,
    })
    .to_string()
}

/// Testo tradotto con nome e azienda del brand.
pub fn localize(key: &str, text: String) -> String {
    if BRANDED {
        rebrand(key, &text, NAME, COMPANY)
    } else {
        text
    }
}

fn rebrand(key: &str, text: &str, name: &str, company: &str) -> String {
    let company = (!SIGNER_KEYS.contains(&key)).then_some(company);
    let rules: [(&str, Option<&str>); 8] = [
        (SYSTEM_NAMES[0], None),
        (SYSTEM_NAMES[1], None),
        ("ClantoDesk", Some(name)),
        ("Clanto Desktop", Some(name)),
        ("Clanto desktop", Some(name)),
        ("Clanto Services srls", company),
        ("Clanto Services", company),
        ("Clanto", company),
    ];
    // Una sola passata: un nome che contiene «Clanto» non viene sostituito di nuovo.
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    'scan: while let Some(ch) = rest.chars().next() {
        for (from, to) in rules {
            if let Some(tail) = rest.strip_prefix(from) {
                out.push_str(to.unwrap_or(from));
                rest = tail;
                continue 'scan;
            }
        }
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebrand_replaces_product_and_company() {
        assert_eq!(
            rebrand("k", "Pronto per la connessione tramite server Clanto", "AcmeDesk", "Acme"),
            "Pronto per la connessione tramite server Acme"
        );
        assert_eq!(
            rebrand("k", "Istanze Clanto Desktop dei server Clanto Services", "AcmeDesk", "Acme"),
            "Istanze AcmeDesk dei server Acme"
        );
        assert_eq!(
            rebrand("k", "The official ClantoDesk client", "ClantoAcme", "Clanto Acme"),
            "The official ClantoAcme client"
        );
        assert_eq!(rebrand("k", "clanto.it", "AcmeDesk", "Acme"), "clanto.it");
    }

    #[test]
    fn rebrand_keeps_system_names_and_signer() {
        assert_eq!(
            rebrand("k", "Approve ClantoDesk Camera; disable ClantoDesk Input", "AcmeDesk", "Acme"),
            "Approve ClantoDesk Camera; disable ClantoDesk Input"
        );
        assert_eq!(
            rebrand("clanto_tampered_error", "ClantoDesk: not signed by Clanto.", "AcmeDesk", "Acme"),
            "AcmeDesk: not signed by Clanto."
        );
    }

    #[test]
    fn branded_build_asks_for_the_partner_release() {
        if BRANDED {
            assert_eq!(update_api_url(), format!("{UPDATE_API}?repo={UPDATE_REPO}"));
            assert_ne!(UPDATE_REPO, "clantodesk-client");
            assert_eq!(localize("k", "Server Clanto".to_owned()), format!("Server {COMPANY}"));
        }
    }

    #[test]
    fn defaults_are_clantodesk() {
        if !BRANDED {
            assert_eq!(NAME, "ClantoDesk");
            assert_eq!(FILE_PREFIX, "clantodesk");
            assert_eq!(UPDATE_REPO, "clantodesk-client");
            assert_eq!(update_api_url(), UPDATE_API);
            assert_eq!(localize("k", "ClantoDesk".to_owned()), "ClantoDesk");
        }
    }
}
