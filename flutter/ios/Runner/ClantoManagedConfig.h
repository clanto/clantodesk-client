// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Funzione Rust (src/clanto/managed_config.rs) chiamata da AppDelegate.swift
// con il JSON di `com.apple.configuration.managed` (configurazione MDM).
void clanto_apply_managed_config(const char *json);
