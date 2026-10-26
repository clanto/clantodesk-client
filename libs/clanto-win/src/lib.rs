// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

//! Controlli nativi di Windows per ClantoDesk, senza PowerShell: firme
//! Authenticode (`trust`) e postura del dispositivo (`posture`). Su altri
//! sistemi il crate è vuoto.

#[cfg(windows)]
pub mod trust;
#[cfg(windows)]
pub mod posture;
