// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Password locale per le impostazioni e, se richiesto, per la schermata host.
// Serve ai dispositivi senza PIN ne' biometria, dove `local_auth` non ha nulla
// da verificare e le impostazioni si aprirebbero a chiunque.

import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:crypto/crypto.dart';
import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:settings_ui/settings_ui.dart';

const String kOptClantoLocalPassword = 'clanto-local-password';
const String kOptClantoLocalPasswordScope = 'clanto-local-password-scope';

/// Cosa protegge la password. `settingsAndHost` copre anche la schermata che
/// mostra ID e password di connessione.
const String kScopeSettings = 'settings';
const String kScopeSettingsAndHost = 'settings+host';

/// Compromesso: abbastanza da rendere inutile un attacco a dizionario, poco
/// abbastanza da non bloccare un SoC lento di un monitor interattivo.
const int _kIterations = 60000;
const int _kSaltBytes = 16;
const int _kKeyBytes = 32;
const int kMinPasswordLength = 6;

class ClantoLocalPassword {
  static bool get isSet => _record().isNotEmpty;

  /// Vero solo se la password c'e' **e** l'utente ha scelto di proteggere anche
  /// la schermata host.
  static bool get guardsHostPage =>
      isSet &&
      bind.getLocalFlutterOption(k: kOptClantoLocalPasswordScope) ==
          kScopeSettingsAndHost;

  static String get scope {
    final s = bind.getLocalFlutterOption(k: kOptClantoLocalPasswordScope);
    return s == kScopeSettingsAndHost ? kScopeSettingsAndHost : kScopeSettings;
  }

  static Future<void> setScope(String value) async {
    await bind.setLocalFlutterOption(
        k: kOptClantoLocalPasswordScope,
        v: value == kScopeSettingsAndHost ? kScopeSettingsAndHost : kScopeSettings);
  }

  static Future<void> set(String plain) async {
    final salt = _randomBytes(_kSaltBytes);
    final dk = _pbkdf2(utf8.encode(plain), salt, _kIterations, _kKeyBytes);
    await bind.setLocalFlutterOption(
        k: kOptClantoLocalPassword,
        v: 'pbkdf2-sha256\$$_kIterations\$${base64Encode(salt)}\$${base64Encode(dk)}');
  }

  static Future<void> clear() async {
    await bind.setLocalFlutterOption(k: kOptClantoLocalPassword, v: '');
  }

  static bool verify(String plain) {
    final parts = _record().split('\$');
    if (parts.length != 4 || parts[0] != 'pbkdf2-sha256') return false;
    final iterations = int.tryParse(parts[1]) ?? 0;
    if (iterations <= 0) return false;
    final List<int> salt, expected;
    try {
      salt = base64Decode(parts[2]);
      expected = base64Decode(parts[3]);
    } catch (_) {
      return false;
    }
    final dk = _pbkdf2(utf8.encode(plain), salt, iterations, expected.length);
    return _constantTimeEquals(dk, expected);
  }

  static String _record() =>
      bind.getLocalFlutterOption(k: kOptClantoLocalPassword);

  static List<int> _randomBytes(int n) {
    final rnd = Random.secure();
    return List<int>.generate(n, (_) => rnd.nextInt(256));
  }

  /// PBKDF2-HMAC-SHA256. Una sola iterazione di blocco: `dkLen` non supera mai
  /// la dimensione dell'hash.
  static List<int> _pbkdf2(
      List<int> password, List<int> salt, int iterations, int dkLen) {
    final hmac = Hmac(sha256, password);
    final block = <int>[...salt, 0, 0, 0, 1];
    var u = hmac.convert(block).bytes;
    final acc = List<int>.from(u);
    for (var i = 1; i < iterations; i++) {
      u = hmac.convert(u).bytes;
      for (var j = 0; j < acc.length; j++) {
        acc[j] ^= u[j];
      }
    }
    return acc.sublist(0, dkLen);
  }

  static bool _constantTimeEquals(List<int> a, List<int> b) {
    if (a.length != b.length) return false;
    var diff = 0;
    for (var i = 0; i < a.length; i++) {
      diff |= a[i] ^ b[i];
    }
    return diff == 0;
  }
}

/// Chiede la password e ritorna se corrisponde. `false` anche se l'utente
/// annulla: chi chiama tratta l'annullamento come accesso negato.
Future<bool> promptClantoLocalPassword(String reason) async {
  final controller = TextEditingController();
  var wrong = false;
  final completer = Completer<bool>();

  void finish(bool ok) {
    if (!completer.isCompleted) completer.complete(ok);
  }

  gFFI.dialogManager.show((setState, close, context) {
    submit() {
      if (ClantoLocalPassword.verify(controller.text)) {
        close();
        finish(true);
      } else {
        setState(() => wrong = true);
      }
    }

    return CustomAlertDialog(
      title: Text(translate('Authentication required')),
      content: Column(mainAxisSize: MainAxisSize.min, children: [
        Text(reason),
        const SizedBox(height: 12),
        TextField(
          controller: controller,
          autofocus: true,
          obscureText: true,
          keyboardType: TextInputType.visiblePassword,
          decoration: InputDecoration(
            labelText: translate('Password'),
            errorText: wrong ? translate('Wrong Password') : null,
          ),
          onSubmitted: (_) => submit(),
        ),
      ]),
      actions: [
        dialogButton('Cancel', onPressed: () {
          close();
          finish(false);
        }, isOutline: true),
        dialogButton('OK', onPressed: submit),
      ],
      onCancel: () {
        close();
        finish(false);
      },
    );
  }, backDismiss: false, clickMaskDismiss: false);

  return completer.future;
}

/// Dialogo per impostare o cambiare la password. Ritorna true se salvata.
Future<bool> setClantoLocalPasswordDialog() async {
  final p1 = TextEditingController();
  final p2 = TextEditingController();
  String? error;
  final completer = Completer<bool>();

  void finish(bool ok) {
    if (!completer.isCompleted) completer.complete(ok);
  }

  gFFI.dialogManager.show((setState, close, context) {
    submit() async {
      if (p1.text.length < kMinPasswordLength) {
        setState(() => error = translate('Too short, at least 6 characters.'));
        return;
      }
      if (p1.text != p2.text) {
        setState(() => error = translate('The confirmation is not identical.'));
        return;
      }
      await ClantoLocalPassword.set(p1.text);
      close();
      showToast(translate('Successful'));
      finish(true);
    }

    return CustomAlertDialog(
      title: Text(translate('Set settings password')),
      content: Column(mainAxisSize: MainAxisSize.min, children: [
        TextField(
          controller: p1,
          autofocus: true,
          obscureText: true,
          keyboardType: TextInputType.visiblePassword,
          decoration: InputDecoration(labelText: translate('Password')),
        ),
        TextField(
          controller: p2,
          obscureText: true,
          keyboardType: TextInputType.visiblePassword,
          decoration: InputDecoration(
            labelText: translate('Confirmation'),
            errorText: error,
          ),
          onSubmitted: (_) => submit(),
        ),
      ]),
      actions: [
        dialogButton('Cancel', onPressed: () {
          close();
          finish(false);
        }, isOutline: true),
        dialogButton('OK', onPressed: submit),
      ],
      onCancel: () {
        close();
        finish(false);
      },
    );
  }, backDismiss: false, clickMaskDismiss: false);

  return completer.future;
}

/// Righe per la sezione "Impostazioni" della pagina mobile. `refresh` rilancia
/// il setState del chiamante.
List<AbstractSettingsTile> clantoLocalPasswordTiles(
    {required VoidCallback refresh}) {
  final isSet = ClantoLocalPassword.isSet;
  return [
    SettingsTile(
      title: Text(translate('Settings password')),
      description: Text(translate(isSet
          ? 'A password is set. It replaces PIN and biometrics.'
          : 'Use a password when the device has no PIN or biometrics.')),
      leading: const Icon(Icons.password),
      onPressed: (context) async {
        if (await setClantoLocalPasswordDialog()) refresh();
      },
    ),
    if (isSet)
      SettingsTile.switchTile(
        title: Text(translate('Also protect the host screen')),
        description:
            Text(translate('Hide the ID and password of this device.')),
        initialValue: ClantoLocalPassword.guardsHostPage,
        onToggle: (v) async {
          await ClantoLocalPassword.setScope(
              v ? kScopeSettingsAndHost : kScopeSettings);
          refresh();
        },
      ),
    if (isSet)
      SettingsTile(
        title: Text(translate('Remove settings password')),
        leading: const Icon(Icons.lock_open),
        onPressed: (context) async {
          // Toglierla richiede conoscerla: altrimenti chi ha il dispositivo in
          // mano la disattiva e apre le impostazioni.
          if (!await promptClantoLocalPassword(
              translate('Authenticate to remove the password'))) {
            return;
          }
          await ClantoLocalPassword.clear();
          await ClantoLocalPassword.setScope(kScopeSettings);
          showToast(translate('Successful'));
          refresh();
        },
      ),
  ];
}
