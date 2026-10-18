// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

use hbb_common::{config::Config, log, password_security as password};
use std::{
    sync::{Mutex, Once},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const OPT_TEMP_PASSWORD_ROTATE_MINUTES: &str = "clanto-temp-password-rotate-minutes";

#[derive(Default)]
struct State {
    password: String,
    observed: bool,
    changed_at: Option<Instant>,
    next_rotation: Option<SystemTime>,
    invalid_option: Option<String>,
}

lazy_static::lazy_static! {
    static ref STATE: Mutex<State> = Mutex::new(State::default());
}

pub fn should_rotate(value: &str, elapsed: Duration) -> bool {
    rotation_minutes(value)
        .map(|minutes| elapsed >= Duration::from_secs(minutes * 60))
        .unwrap_or(false)
}

pub fn next_rotation_epoch() -> Option<u64> {
    STATE
        .lock()
        .unwrap()
        .next_rotation
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
}

pub fn start() {
    static START: Once = Once::new();
    START.call_once(|| {
        refresh();
        tokio::spawn(async {
            let mut timer = tokio::time::interval(Duration::from_secs(15));
            loop {
                timer.tick().await;
                refresh();
            }
        });
    });
}

fn rotation_minutes(value: &str) -> Option<u64> {
    value
        .parse::<u64>()
        .ok()
        .filter(|minutes| (1..=1440).contains(minutes))
}

fn rotation_disabled(value: &str) -> bool {
    value.is_empty() || value.parse::<u64>().ok() == Some(0)
}

fn refresh() {
    let value = Config::get_option(OPT_TEMP_PASSWORD_ROTATE_MINUTES);
    let Some(minutes) = rotation_minutes(&value) else {
        let mut state = STATE.lock().unwrap();
        state.password.clear();
        state.observed = false;
        state.changed_at = None;
        state.next_rotation = None;
        if !rotation_disabled(&value) && state.invalid_option.as_deref() != Some(&value) {
            log::warn!(
                "Invalid temporary password rotation interval '{}'; rotation disabled",
                value
            );
            state.invalid_option = Some(value);
        } else if rotation_disabled(&value) {
            state.invalid_option = None;
        }
        return;
    };

    if !password::temporary_enabled() {
        let mut state = STATE.lock().unwrap();
        state.password.clear();
        state.observed = false;
        state.changed_at = None;
        state.next_rotation = None;
        state.invalid_option = None;
        return;
    }

    let current_password = password::temporary_password();
    let now = Instant::now();
    let now_wall = SystemTime::now();
    let mut rotate = false;
    {
        let mut state = STATE.lock().unwrap();
        state.invalid_option = None;
        if !state.observed || state.password != current_password {
            state.password = current_password.clone();
            state.observed = true;
            state.changed_at = Some(now);
        }
        if let Some(changed_at) = state.changed_at {
            if should_rotate(&value, changed_at.elapsed()) {
                rotate = true;
            } else if let Some(remaining) =
                Duration::from_secs(minutes * 60).checked_sub(changed_at.elapsed())
            {
                state.next_rotation = now_wall.checked_add(remaining);
            }
        }
    }

    if !rotate {
        return;
    }

    password::update_temporary_password();
    let updated_password = password::temporary_password();
    if updated_password == current_password {
        return;
    }

    let mut state = STATE.lock().unwrap();
    state.password = updated_password;
    state.changed_at = Some(Instant::now());
    state.next_rotation = SystemTime::now().checked_add(Duration::from_secs(minutes * 60));
}

#[cfg(test)]
mod tests {
    use super::should_rotate;
    use std::time::Duration;

    #[test]
    fn zero_disables_rotation() {
        assert!(!should_rotate("0", Duration::from_secs(24 * 60 * 60)));
    }

    #[test]
    fn waits_for_the_configured_interval() {
        assert!(!should_rotate("10", Duration::from_secs(9 * 60)));
        assert!(should_rotate("10", Duration::from_secs(10 * 60)));
    }

    #[test]
    fn invalid_value_disables_rotation() {
        assert!(!should_rotate("ten", Duration::from_secs(24 * 60 * 60)));
    }
}
