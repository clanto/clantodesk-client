// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Codice specifico di ClantoDesk.
//
// Regola: la logica sta qui, nei file upstream ci va solo la riga di chiamata.
// Una riga aggiunta si auto-mergia quasi sempre; trenta righe intrecciate nel
// corpo di una funzione upstream conflittano a ogni aggiornamento.

pub mod audit_sink;
pub mod config;
