// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/consts.dart';

class ClantoChatNotification {
  static const _channel = MethodChannel('mChannel');

  /// Il ruolo controller non avvia MainService, che e' l'unico punto in cui
  /// upstream chiede POST_NOTIFICATIONS: senza questa richiesta le notifiche
  /// di chat vengono scartate in silenzio su Android 13+.
  static Future<void> ensurePermission() async {
    if (!isAndroid || androidVersion < 33) return;
    if (await AndroidPermissionManager.check(kAndroid13Notification)) return;
    await AndroidPermissionManager.request(kAndroid13Notification);
  }

  static Future<void> publish(String text) async {
    try {
      await _channel.invokeMethod<void>(
        'clanto_chat_notification',
        {'text': text},
      );
    } catch (_) {}
  }
}
