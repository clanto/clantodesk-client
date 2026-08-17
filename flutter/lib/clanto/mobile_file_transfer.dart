// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Coordinamento multipiattaforma del trasferimento file mobile ClantoDesk.
// La selezione usa il picker di sistema; la pubblicazione dei file ricevuti è
// delegata all'integrazione nativa appropriata per Android o iOS.

import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_hbb/clanto/android_file_transfer.dart';
import 'package:flutter_hbb/clanto/ios_file_transfer.dart';

class ClantoPickedFile {
  const ClantoPickedFile({
    required this.name,
    required this.path,
    required this.size,
  });

  final String name;
  final String path;
  final int size;
}

class MobileFileTransfer {
  /// Apre il selettore documenti del sistema e restituisce soltanto file con
  /// un percorso locale leggibile dal core di trasferimento.
  static Future<List<ClantoPickedFile>> pickFiles() async {
    final result = await FilePicker.platform.pickFiles(allowMultiple: true);
    if (result == null) {
      return const [];
    }
    return result.files
        .where((file) => file.path != null && file.path!.isNotEmpty)
        .map((file) => ClantoPickedFile(
              name: file.name,
              path: file.path!,
              size: file.size,
            ))
        .toList(growable: false);
  }

  /// Rende disponibile all'utente un file ricevuto dal computer remoto.
  /// Android lo pubblica in Download; iOS presenta il selettore dell'app File
  /// per scegliere una destinazione locale, iCloud o di terze parti.
  static Future<void> publishReceivedFile(String path, String name) async {
    if (defaultTargetPlatform == TargetPlatform.android) {
      await AndroidFileTransfer.publishToDownloads(path, name);
    } else if (defaultTargetPlatform == TargetPlatform.iOS) {
      await IosFileTransfer.exportToFiles(path, name);
    }
  }
}
