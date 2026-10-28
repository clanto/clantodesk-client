// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Client HTTP minimo verso /api/clanto/v1 con il token di accesso dell'account
// e contenitore comune (dialogo su desktop, pagina su mobile) delle schermate.

import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_hbb/clanto/managed_data.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';

import '../utils/http_service.dart' as http;

class ClantoApiResult {
  final int status;
  final Map<String, dynamic> json;
  final String? errorCode;
  final bool networkError;

  const ClantoApiResult(this.status, this.json,
      {this.errorCode, this.networkError = false});

  bool get ok => !networkError && status >= 200 && status < 300;
  bool get unauthorized => status == 401;

  /// Messaggio tradotto per l'utente.
  String get message {
    if (networkError) return translate('clanto_err_network');
    final key = clantoErrorKey(errorCode, status);
    if (key != null) return translate(key);
    return '${translate('clanto_err_generic')}: ${errorCode ?? status}';
  }
}

/// Funzionalità account visibili: account abilitato e login effettuato.
bool clantoAccountFeaturesAvailable() =>
    !bind.isDisableAccount() && gFFI.userModel.userName.value.isNotEmpty;

Future<ClantoApiResult> clantoApi(
  String method,
  String path, {
  Map<String, String>? query,
  Map<String, dynamic>? body,
}) async {
  try {
    final base = Uri.parse(await bind.mainGetApiServer());
    final uri = Uri(
      scheme: base.scheme,
      host: base.host,
      port: base.hasPort ? base.port : null,
      path: '$kClantoApiPrefix$path',
      queryParameters: (query == null || query.isEmpty) ? null : query,
    );
    final headers = {
      ...getHttpHeaders(),
      'Content-Type': 'application/json',
    };
    final resp = method == 'POST'
        ? await http.post(uri, headers: headers, body: jsonEncode(body ?? {}))
        : await http.get(uri, headers: headers);
    Map<String, dynamic> json = {};
    final text = decode_http_response(resp);
    if (text.trim().isNotEmpty) {
      try {
        final d = jsonDecode(text);
        if (d is Map<String, dynamic>) json = d;
      } catch (_) {}
    }
    final err = json['error'];
    return ClantoApiResult(resp.statusCode, json,
        errorCode: err is String ? err : null);
  } catch (_) {
    return const ClantoApiResult(0, {}, networkError: true);
  }
}

/// Testo di una durata, es. "15 min" o "2 h".
String clantoDurationLabel(int seconds) {
  final p = clantoDurationParts(seconds);
  return '${p.value} ${translate(p.hours ? 'clanto_unit_hours' : 'clanto_unit_minutes')}';
}

/// Mostra [child] in un dialogo (desktop) o in una pagina (mobile).
Future<void> clantoShowScreen(BuildContext context,
    {required String title, required Widget child}) {
  if (isDesktop) {
    return showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: Row(children: [
          Expanded(child: Text(title)),
          IconButton(
              icon: const Icon(Icons.close),
              onPressed: () => Navigator.of(context).pop()),
        ]),
        content: SizedBox(width: 720, height: 520, child: child),
      ),
    );
  }
  return Navigator.of(context).push(MaterialPageRoute<void>(
    builder: (context) => Scaffold(
      appBar: AppBar(title: Text(title)),
      body: SafeArea(child: child),
    ),
  ));
}

/// Riquadro con messaggio d'errore e pulsante "Riprova".
class ClantoErrorBox extends StatelessWidget {
  const ClantoErrorBox({super.key, required this.message, this.onRetry});

  final String message;
  final VoidCallback? onRetry;

  @override
  Widget build(BuildContext context) => Center(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            Icon(Icons.error_outline,
                color: Theme.of(context).colorScheme.error),
            const SizedBox(height: 8),
            Text(message, textAlign: TextAlign.center),
            if (onRetry != null)
              TextButton(onPressed: onRetry, child: Text(translate('Retry'))),
          ]),
        ),
      );
}
