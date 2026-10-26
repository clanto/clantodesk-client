// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Codice specifico di ClantoDesk.
//
// Regola: la logica sta qui, nei file upstream ci va solo la riga di chiamata.
// Una riga aggiunta si auto-mergia quasi sempre; trenta righe intrecciate nel
// corpo di una funzione upstream conflittano a ogni aggiornamento.

pub mod access_hours;
pub mod audit_sink;
pub mod config;
pub mod config_import;
pub mod file_scan;
pub mod forwarding;
pub mod gpo;
pub mod host_watermark;
pub mod id_permissions;
pub mod keystore;
pub mod managed_config;
pub mod outgoing;
pub mod posture;
pub mod server_sync;
pub mod session_log;
pub mod settings_lock;
pub mod temp_password;
pub mod trusted_controllers;
pub mod signature;
pub mod ticket;
pub mod two_factor_policy;
pub mod two_factor_store;
pub mod watermark;

/// Controlli dell'host prima della password: fasce orarie, 2FA illeggibile,
/// 2FA richiesto dalla policy. Motivo già tradotto per il controller.
pub fn login_block() -> Option<String> {
    access_hours::login_block().or_else(two_factor_policy::login_block)
}
