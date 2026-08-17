// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Integrazione Android specifica di ClantoDesk per lo scoped storage. I file
// ricevuti vengono resi pubblici in Download tramite MediaStore.

import 'package:flutter/services.dart';

class AndroidFileTransfer {
  static const _channel = MethodChannel('mChannel');

  /// Copia un file dal sandbox dell'app nella raccolta pubblica Download.
  static Future<String> publishToDownloads(String path, String name) async {
    final publishedPath = await _channel.invokeMethod<String>(
      'clanto_publish_download',
      {'path': path, 'name': name},
    );
    if (publishedPath == null || publishedPath.isEmpty) {
      throw PlatformException(
        code: 'publish_failed',
        message: 'Android did not return the published download',
      );
    }
    return publishedPath;
  }
}
