// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

#pragma once

#include <string>

// Nomi dei .exe e .dll di `dir` (senza sottocartelle) non firmati da Clanto
// (EKU dell'identità Azure Trusted Signing) né da Microsoft; vuoto se tutti
// integri. `eku` vuoto: nessun controllo (build di sviluppo).
std::wstring ClantoTamperedFiles(const std::wstring& dir, const char* eku);

// Controllo all'avvio sulla cartella dell'eseguibile. Se un binario è
// manomesso registra l'evento 61180, mostra l'errore (se c'è un desktop) e
// restituisce false: il processo non deve proseguire.
bool ClantoIntegrityOk();
