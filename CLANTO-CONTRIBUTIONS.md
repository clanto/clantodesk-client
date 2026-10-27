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

- `src/clanto/access_hours.rs`
- `src/clanto/audit_sink.rs`
- `src/clanto/config.rs`
- `src/clanto/config_import.rs`
- `src/clanto/file_scan.rs`
- `src/clanto/forwarding/camera_capture.rs`
- `src/clanto/forwarding/camera_capture_mobile.rs`
- `src/clanto/forwarding/camera_receiver.rs`
- `src/clanto/forwarding/camera_sink.rs`
- `src/clanto/forwarding/catalog.rs`
- `src/clanto/forwarding/controller.rs`
- `src/clanto/forwarding/engine.rs`
- `src/clanto/forwarding/host.rs`
- `src/clanto/forwarding/media.rs`
- `src/clanto/forwarding/mod.rs`
- `src/clanto/forwarding/policy.rs`
- `src/clanto/gpo.rs`
- `src/clanto/host_watermark.rs`
- `src/clanto/host_watermark/android.rs`
- `src/clanto/host_watermark/linux.rs`
- `src/clanto/host_watermark/macos.rs`
- `src/clanto/host_watermark/windows.rs`
- `src/clanto/id_permissions.rs`
- `src/clanto/keystore.rs`
- `src/clanto/managed_config.rs`
- `src/clanto/mod.rs`
- `src/clanto/outgoing.rs`
- `src/clanto/posture.rs`
- `src/clanto/server_sync.rs`
- `src/clanto/session_log.rs`
- `src/clanto/settings_lock.rs`
- `src/clanto/signature.rs`
- `src/clanto/temp_password.rs`
- `src/clanto/ticket.rs`
- `src/clanto/trusted_controllers.rs`
- `src/clanto/two_factor_policy.rs`
- `src/clanto/two_factor_store.rs`
- `src/clanto/watermark.rs`
- `src/platform/linux_gtk_clipboard.rs`
- `flutter/lib/clanto/about_links.dart`
- `flutter/lib/clanto/access_hours.dart`
- `flutter/lib/clanto/accessibility_disclosure.dart`
- `flutter/lib/clanto/android_file_transfer.dart`
- `flutter/lib/clanto/audit_settings.dart`
- `flutter/lib/clanto/chat_notification.dart`
- `flutter/lib/clanto/file_transfer_security.dart`
- `flutter/lib/clanto/forwarding.dart`
- `flutter/lib/clanto/id_permissions.dart`
- `flutter/lib/clanto/id_permissions_data.dart`
- `flutter/lib/clanto/inbound_security.dart`
- `flutter/lib/clanto/ios_file_transfer.dart`
- `flutter/lib/clanto/mobile_file_transfer.dart`
- `flutter/lib/clanto/outgoing_security.dart`
- `flutter/lib/clanto/posture_login_error.dart`
- `flutter/lib/clanto/privacy_prompt.dart`
- `flutter/lib/clanto/protected_device.dart`
- `flutter/lib/clanto/server_status.dart`
- `flutter/lib/clanto/session_report.dart`
- `flutter/lib/clanto/session_report_data.dart`
- `flutter/lib/clanto/settings_auth.dart`
- `flutter/lib/clanto/settings_lock.dart`
- `flutter/lib/clanto/settings_password.dart`
- `flutter/lib/clanto/tamper_check.dart`
- `flutter/lib/clanto/temp_password_rotation.dart`
- `flutter/lib/clanto/trusted_controllers.dart`
- `flutter/lib/clanto/two_factor_policy.dart`
- `flutter/lib/clanto/watermark.dart`
- `flutter/lib/clanto/watermark_data.dart`
- `flutter/android/app/src/main/kotlin/it/clanto/clantodesk/clanto/ClantoCamera.kt`
- `flutter/android/app/src/main/kotlin/it/clanto/clantodesk/clanto/ClantoChatNotifier.kt`
- `flutter/android/app/src/main/kotlin/it/clanto/clantodesk/clanto/ClantoDownloads.kt`
- `flutter/android/app/src/main/kotlin/it/clanto/clantodesk/clanto/ClantoHostWatermark.kt`
- `flutter/android/app/src/main/kotlin/it/clanto/clantodesk/clanto/ClantoManagedConfig.kt`
- `flutter/android/app/src/main/kotlin/it/clanto/clantodesk/clanto/ClantoUserKeyStore.kt`
- `flutter/android/app/src/main/res/xml/app_restrictions.xml`
- `flutter/android/app/src/main/res/values/clanto_mdm.xml`, `values-it/clanto_mdm.xml`
- `flutter/ios/Runner/ClantoCamera.swift`, `ClantoCamera.h`
- `flutter/ios/Runner/ClantoFileExporter.swift`
- `flutter/ios/Runner/ClantoManagedConfig.h`
- `flutter/windows/runner/clanto_integrity.cpp`, `clanto_integrity.h`
- `libs/clanto-vcam/Cargo.toml`, `dll/Cargo.toml`
- `libs/clanto-vcam/src/lib.rs`, `frame.rs`, `shared.rs`, `control.rs`, `mac.rs`
- `libs/clanto-vcam/dll/src/lib.rs`, `source.rs`
- `libs/clanto-win/Cargo.toml`, `src/lib.rs`, `src/trust.rs`, `src/posture.rs`
- `flutter/macos/CameraExtension/main.swift`, `Info.plist`, `CameraExtension.entitlements`
- `.github/scripts/build-macos-camera-extension.sh`
- `docker/Dockerfile.android`, `docker/android-build.sh`, `docker/android-inner.sh`
- `docker/e2e/e2e_harness.rs`
- `api/clantodesk.php`, `api/clantodesk-posture.php`

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
- `libs/clanto-vcam/` e `libs/clanto-win/` per i crate separati (la DLL della
  camera virtuale deve essere un binario a sé; le API native Windows stanno in
  un crate proprio);
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
