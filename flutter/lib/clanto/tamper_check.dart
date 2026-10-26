// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Eseguibile manomesso (src/clanto/signature.rs), macOS: se il bundle non è
// firmato dal team Clanto l'interfaccia mostra l'errore e si chiude. Su Windows
// lo fanno l'exe e librustdesk prima che parta l'interfaccia.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';

Future<void> clantoTamperCheck(BuildContext context) async {
  if (!Platform.isMacOS) return;
  final message = await bind.mainClantoTamperCheck();
  if (message.isEmpty || !context.mounted) return;
  await showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (context) => PopScope(
      canPop: false,
      child: AlertDialog(
        title: Text(translate('Tampered executable')),
        content: SelectableText(message),
        actions: [
          TextButton(
            onPressed: () => exit(1),
            child: Text(translate('Close')),
          ),
        ],
      ),
    ),
  );
  exit(1);
}
