// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Fasce orarie di accesso all'host (`clanto-access-hours`, anche via GPO).
//!
//! Vuota = accesso a qualsiasi ora. Altrimenti voci separate da `;`, ciascuna
//! `giorni orari`: giorni `mon`..`sun` (o `lun`..`dom`), singoli, a intervallo
//! (`mon-fri`) o in elenco (`sat,sun`); orari `HH:MM-HH:MM` separati da
//! virgola, `24:00` come fine giornata. Una fascia con fine prima dell'inizio
//! prosegue nel giorno dopo (`fri 22:00-06:00`). `none` = nessuna fascia.
//! Ora locale dell'host.
//! Fuori dalle fasce ogni accesso è rifiutato; una voce non valida rende
//! l'opzione non valida e l'accesso è rifiutato (fail-closed).

use chrono::{Datelike, Local, Timelike};
use hbb_common::{config::Config, log};

pub const OPT_ACCESS_HOURS: &str = "clanto-access-hours";
const LOGIN_ERROR: &str = "clanto_access_hours_error";
const DAY_MINUTES: u32 = 24 * 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    /// Lunedì = 0.
    days: [bool; 7],
    start: u32,
    end: u32,
}

fn day(name: &str) -> Option<usize> {
    const EN: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    const IT: [&str; 7] = ["lun", "mar", "mer", "gio", "ven", "sab", "dom"];
    let name = name.trim().to_ascii_lowercase();
    EN.iter().position(|d| *d == name).or_else(|| IT.iter().position(|d| *d == name))
}

fn days(spec: &str) -> Option<[bool; 7]> {
    let mut out = [false; 7];
    for part in spec.split(',') {
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (day(a)?, day(b)?);
                let mut d = a;
                loop {
                    out[d] = true;
                    if d == b {
                        break;
                    }
                    d = (d + 1) % 7;
                }
            }
            None => out[day(part)?] = true,
        }
    }
    Some(out)
}

fn minutes(hhmm: &str) -> Option<u32> {
    let (h, m) = hhmm.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    let total = h.checked_mul(60)?.checked_add(m)?;
    (m < 60 && total <= DAY_MINUTES).then_some(total)
}

/// `None` se una voce non è valida.
pub fn parse(value: &str) -> Option<Vec<Window>> {
    let mut windows = Vec::new();
    for entry in value.split(';').map(str::trim).filter(|e| !e.is_empty()) {
        if entry.eq_ignore_ascii_case("none") {
            continue;
        }
        let (day_spec, ranges) = entry.split_once(char::is_whitespace)?;
        let days = days(day_spec)?;
        for range in ranges.split(',') {
            let (start, end) = range.trim().split_once('-')?;
            let (start, end) = (minutes(start)?, minutes(end)?);
            if start == end || start == DAY_MINUTES {
                return None;
            }
            windows.push(Window { days, start, end });
        }
    }
    Some(windows)
}

/// `weekday`: lunedì = 0; `minute`: minuti dalla mezzanotte.
pub fn allowed_at(windows: &[Window], weekday: usize, minute: u32) -> bool {
    let yesterday = (weekday + 6) % 7;
    windows.iter().any(|w| {
        if w.start < w.end {
            w.days[weekday] && minute >= w.start && minute < w.end
        } else {
            (w.days[weekday] && minute >= w.start) || (w.days[yesterday] && minute < w.end)
        }
    })
}

/// Motivo del rifiuto, già tradotto, se adesso l'accesso non è consentito.
pub fn login_block() -> Option<String> {
    let value = Config::get_option(OPT_ACCESS_HOURS);
    if value.trim().is_empty() {
        return None;
    }
    let now = Local::now();
    let allowed = match parse(&value) {
        Some(windows) => allowed_at(
            &windows,
            now.weekday().num_days_from_monday() as usize,
            now.hour() * 60 + now.minute(),
        ),
        None => {
            log::error!("Invalid {OPT_ACCESS_HOURS}: every connection is rejected");
            false
        }
    };
    if allowed {
        return None;
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    return Some(crate::lang::translate(LOGIN_ERROR.to_owned()));
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return Some(crate::lang::translate_locale(LOGIN_ERROR.to_owned(), ""));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn office_hours() {
        let w = parse("mon-fri 08:00-13:00,14:00-18:00; sat 09:00-12:00").unwrap();
        assert!(allowed_at(&w, 0, 8 * 60));
        assert!(!allowed_at(&w, 0, 13 * 60 + 30));
        assert!(!allowed_at(&w, 4, 18 * 60));
        assert!(allowed_at(&w, 5, 10 * 60));
        assert!(!allowed_at(&w, 6, 10 * 60));
    }

    #[test]
    fn overnight_and_italian_days() {
        let w = parse("ven 22:00-06:00").unwrap();
        assert!(allowed_at(&w, 4, 23 * 60));
        assert!(allowed_at(&w, 5, 5 * 60));
        assert!(!allowed_at(&w, 5, 7 * 60));
        assert!(!allowed_at(&w, 3, 23 * 60));
        let w = parse("sat-mon 00:00-24:00").unwrap();
        assert!(allowed_at(&w, 6, 0) && allowed_at(&w, 0, 23 * 60 + 59));
        assert!(!allowed_at(&w, 1, 12 * 60));
    }

    #[test]
    fn invalid_values_are_rejected() {
        for bad in ["mon", "xyz 08:00-09:00", "mon 08:00", "mon 25:00-26:00", "mon 08:00-08:00", "mon 8:61-9:00"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert_eq!(parse(""), Some(vec![]));
        assert!(!allowed_at(&parse("none").unwrap(), 0, 600));
    }
}
