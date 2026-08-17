# Contributi ClantoDesk: provenienza e licenze

Questo documento identifica il materiale sviluppato specificamente da Clanto
Services srls e distingue tale materiale dal codice derivato da RustDesk e dai
componenti di terze parti.

## Concessione sui contributi Clanto

Nella misura in cui Clanto Services srls ne detiene il copyright, i contributi
Clanto identificati in questo documento e nella cronologia Git sono disponibili,
a scelta del destinatario, secondo:

- GNU Affero General Public License v3.0 soltanto (`AGPL-3.0-only`); oppure
- Apache License 2.0 (`Apache-2.0`).

Questa concessione non modifica la licenza del materiale RustDesk o di terze
parti e non attribuisce a Clanto diritti che non possiede. Il programma combinato
ClantoDesk resta distribuito sotto `AGPL-3.0-only`; la scelta Apache riguarda
soltanto contributi separabili sui quali Clanto detiene tutti i diritti necessari.
In particolare, consente al team RustDesk e ad altri destinatari di riusare tali
contributi separabili anche in prodotti distribuiti con una licenza compatibile
con Apache-2.0.

I file interamente Clanto riportano l'identificatore SPDX
`AGPL-3.0-only OR Apache-2.0`:

- `src/clanto/audit_sink.rs`
- `src/clanto/config.rs`
- `src/clanto/mod.rs`
- `flutter/lib/clanto/accessibility_disclosure.dart`
- `flutter/lib/clanto/android_file_transfer.dart`
- `flutter/lib/clanto/audit_settings.dart`
- `flutter/lib/clanto/ios_file_transfer.dart`
- `flutter/lib/clanto/mobile_file_transfer.dart`
- `flutter/android/app/src/main/kotlin/it/clanto/clantodesk/clanto/ClantoDownloads.kt`
- `flutter/ios/Runner/ClantoFileExporter.swift`

L'elenco deve essere aggiornato quando viene aggiunto, rimosso o riclassificato
un modulo. L'intestazione SPDX del singolo file prevale su questo inventario in
caso di discrepanza.

## File derivati e hook nel codice upstream

Molte funzionalità richiedono piccoli hook, adattamenti o integrazioni nei file
originari di RustDesk. Questi file non vengono riclassificati come interamente
Clanto e restano sotto la licenza upstream applicabile, normalmente
`AGPL-3.0-only`. Fra gli esempi attuali:

- `src/common.rs`, `src/core_main.rs`, `src/flutter_ffi.rs` e
  `src/server/connection.rs` per gli hook Rust;
- `flutter/lib/models/`, `flutter/lib/mobile/`, `flutter/lib/desktop/` e i file
  Flutter condivisi per l'integrazione nell'interfaccia;
- `MainActivity.kt` e `InputService.kt` per gli hook Android;
- `AppDelegate.swift`, `Info.plist` e il progetto Xcode per l'hook e le
  dichiarazioni iOS;
- workflow, manifest, installer, metadati e risorse nei percorsi richiesti dai
  rispettivi sistemi di build.

La presenza del nome Clanto nel package Android o nel bundle identifier non
rende automaticamente un file codice originale Clanto. Provenienza, cronologia
Git e intestazioni di licenza vanno valutate insieme.

## Organizzazione del codice

La logica nuova e separabile viene collocata vicino all'ecosistema che la usa:

- `src/clanto/` per Rust;
- `flutter/lib/clanto/` per Dart/Flutter;
- `it.clanto.clantodesk.clanto` per Kotlin/Android;
- `flutter/ios/Runner/` per sorgenti Swift che devono appartenere al target
  Xcode.

Non viene creata una cartella `clanto/` unica alla radice: Flutter, Cargo,
Gradle, gli installer e i sistemi operativi richiedono percorsi specifici. Nei
file upstream devono rimanere, per quanto possibile, solo import e hook minimi.

## Marchi e asset

La licenza di copyright di un file non equivale a una licenza di marchio. Il
nome ClantoDesk, i loghi e gli altri elementi distintivi restano soggetti alla
policy in [TRADEMARKS.md](TRADEMARKS.md). Nessuna delle licenze software concede
il diritto di presentare un fork come prodotto ufficiale Clanto.

I testi completi delle licenze sono disponibili in [LICENCE](LICENCE) per AGPL
v3 e in [LICENSES/Apache-2.0.txt](LICENSES/Apache-2.0.txt) per Apache 2.0.
