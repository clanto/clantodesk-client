// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'package:local_auth/local_auth.dart';

/// Protects mobile settings with the authentication configured on the device.
class ClantoSettingsAuth {
  static final LocalAuthentication _auth = LocalAuthentication();

  static Future<bool> authenticate(String reason) async {
    try {
      if (!await _auth.isDeviceSupported()) return false;
      return await _auth.authenticate(
        localizedReason: reason,
        options: const AuthenticationOptions(
          biometricOnly: false,
          stickyAuth: true,
          useErrorDialogs: true,
        ),
      );
    } catch (_) {
      return false;
    }
  }
}
