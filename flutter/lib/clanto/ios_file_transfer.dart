// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Integrazione iOS specifica di ClantoDesk. Il codice Swift presenta il
// document picker e accoda le esportazioni quando arrivano più file insieme.

import 'package:flutter/services.dart';

class IosFileTransfer {
  static const _channel = MethodChannel('mChannel');

  /// Chiede all'utente dove conservare il file ricevuto. Se il picker viene
  /// annullato, il file rimane comunque nella cartella Documents di ClantoDesk.
  static Future<void> exportToFiles(String path, String name) async {
    await _channel.invokeMethod<String>(
      'clanto_export_received_file',
      {'path': path, 'name': name},
    );
  }
}
