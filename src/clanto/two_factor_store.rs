// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Salvataggio del segreto 2FA e del token del bot Telegram.
//!
//! Entrambi sono cifrati con la chiave macchina (`machine.key`) di chi li
//! salva. Su desktop il servizio gira con un altro utente (SYSTEM, root) e
//! quindi con un'altra chiave: se cifrasse la UI, il servizio non potrebbe
//! leggere il segreto e accetterebbe la sola password. La UI manda quindi il
//! valore in chiaro sull'IPC locale e la cifratura avviene nel servizio.
//! Un segreto presente ma illeggibile blocca gli accessi (fail-closed).

use hbb_common::{config::Config, log};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use hbb_common::{bail, ResultType};

pub const IPC_2FA: &str = "clanto-2fa";
pub const IPC_BOT: &str = "clanto-bot";
pub const IPC_STATE: &str = "clanto-2fa-state";
const LOGIN_ERROR: &str = "clanto_2fa_unreadable_error";

const OPT_2FA: &str = "2fa";
const OPT_BOT: &str = "bot";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Missing,
    Valid,
    Unreadable,
}

impl State {
    fn code(self) -> char {
        match self {
            State::Missing => 'N',
            State::Valid => 'Y',
            State::Unreadable => 'E',
        }
    }

    fn from_code(c: Option<char>) -> Option<Self> {
        match c? {
            'N' => Some(State::Missing),
            'Y' => Some(State::Valid),
            'E' => Some(State::Unreadable),
            _ => None,
        }
    }
}

fn state_of(option: &str, readable: impl FnOnce() -> bool) -> State {
    if Config::get_option(option).is_empty() {
        State::Missing
    } else if readable() {
        State::Valid
    } else {
        State::Unreadable
    }
}

/// Stato del 2FA nella configurazione di questo processo.
pub fn local_2fa() -> State {
    state_of(OPT_2FA, || crate::auth_2fa::get_2fa(None).is_some())
}

/// Stato del bot Telegram nella configurazione di questo processo.
pub fn local_bot() -> State {
    state_of(OPT_BOT, || {
        matches!(crate::auth_2fa::TelegramBot::get(), Ok(Some(_)))
    })
}

/// Motivo del rifiuto se il 2FA è configurato ma il servizio non riesce a
/// decifrarlo: senza questo controllo basterebbe la password.
pub fn login_block() -> Option<String> {
    if local_2fa() != State::Unreadable {
        return None;
    }
    log::error!("2FA secret present but unreadable with this machine key");
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    return Some(crate::lang::translate(LOGIN_ERROR.to_owned()));
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return Some(crate::lang::translate_locale(LOGIN_ERROR.to_owned(), ""));
}

/// Lato servizio: lettura via IPC (`Data::Config` senza valore).
pub fn on_ipc_get(name: &str) -> Option<String> {
    (name == IPC_STATE).then(|| [local_2fa().code(), local_bot().code()].iter().collect())
}

/// Lato servizio: scrittura via IPC. `None` se il nome non è nostro,
/// altrimenti l'esito da restituire alla UI.
pub fn on_ipc_set(name: &str, plain: &str) -> Option<bool> {
    let option = match name {
        IPC_2FA => OPT_2FA,
        IPC_BOT => OPT_BOT,
        _ => return None,
    };
    if !crate::clanto::settings_lock::user_write_allowed_now() {
        log::trace!("{}", crate::clanto::settings_lock::MANAGED_SETTINGS_MESSAGE);
        return Some(false);
    }
    let stored = if plain.is_empty() {
        Ok(String::new())
    } else if option == OPT_2FA {
        crate::auth_2fa::encrypt_plain_2fa(plain)
    } else {
        crate::auth_2fa::TelegramBot::encrypt_plain(plain)
    };
    match stored {
        Ok(value) => {
            Config::set_option(option.to_owned(), value);
            log::info!("{} updated", option);
            Some(true)
        }
        Err(err) => {
            log::error!("Failed to store {}: {}", option, err);
            Some(false)
        }
    }
}

/// Lato UI desktop: affida al servizio il valore in chiaro. Se il servizio
/// non risponde la UI è anche l'host (portable) e cifra in locale.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn save(name: &str, plain: String) -> bool {
    match send(name, plain.clone()) {
        Ok(ok) => ok,
        Err(err) => {
            log::warn!("2FA store via service failed, storing locally: {}", err);
            on_ipc_set(name, &plain).unwrap_or(false)
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn send(name: &str, plain: String) -> ResultType<bool> {
    use crate::ipc::Data;
    let runtime = hbb_common::tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let mut c = crate::ipc::connect(1000, "").await?;
        c.send(&Data::Config((name.to_owned(), Some(plain)))).await?;
        if let Some(Data::Config((reply, Some(v)))) = c.next_timeout(1000).await? {
            if reply == name {
                return Ok(v == "Y");
            }
        }
        bail!("no reply from the service")
    })
}

/// Lato UI desktop: stato `(2fa, bot)` visto dal servizio, che è quello che
/// conta per gli accessi. Senza servizio vale la configurazione locale.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn service_state() -> (State, State) {
    if let Ok(Some(v)) = crate::ipc::get_config(IPC_STATE) {
        let mut c = v.chars();
        if let (Some(tfa), Some(bot)) = (State::from_code(c.next()), State::from_code(c.next())) {
            return (tfa, bot);
        }
    }
    (local_2fa(), local_bot())
}

/// Lato UI desktop: le versioni precedenti cifravano nella UI. Se il servizio
/// non legge il valore ma la UI sì, lo reinvia in chiaro perché il servizio
/// lo cifri con la propria chiave.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn repair() {
    if crate::platform::is_root() {
        return;
    }
    let (tfa, bot) = service_state();
    if tfa == State::Unreadable {
        if let Some(plain) = crate::auth_2fa::plain_2fa(&Config::get_option(OPT_2FA)) {
            log::info!("Re-storing the 2FA secret through the service: {}", save(IPC_2FA, plain));
        }
    }
    if bot == State::Unreadable {
        if let Ok(Some(b)) = crate::auth_2fa::TelegramBot::get() {
            log::info!("Re-storing the Telegram bot through the service: {}", save(IPC_BOT, b.plain()));
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn repair_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::thread::spawn(repair);
    });
}

#[cfg(test)]
mod tests {
    use super::State;

    #[test]
    fn state_codes_round_trip() {
        for s in [State::Missing, State::Valid, State::Unreadable] {
            assert_eq!(State::from_code(Some(s.code())), Some(s));
        }
        assert_eq!(State::from_code(Some('x')), None);
        assert_eq!(State::from_code(None), None);
    }

    #[test]
    fn rejects_malformed_plain_values() {
        assert_eq!(super::on_ipc_set("other", "x"), None);
        assert!(crate::auth_2fa::TelegramBot::encrypt_plain("token-only").is_err());
        assert!(crate::auth_2fa::TelegramBot::encrypt_plain("
token").is_err());
        let empty = r#"{"name":"1","secret":[],"digits":6,"created_at":0}"#;
        assert!(crate::auth_2fa::encrypt_plain_2fa(empty).is_err());
        assert!(crate::auth_2fa::encrypt_plain_2fa("not json").is_err());
    }
}
