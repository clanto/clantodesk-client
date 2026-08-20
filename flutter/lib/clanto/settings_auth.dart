// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'package:flutter_hbb/common.dart' show translate;
import 'package:local_auth/local_auth.dart';
// ignore: depend_on_referenced_packages
import 'package:local_auth_android/local_auth_android.dart'
    show AndroidAuthMessages;
// ignore: depend_on_referenced_packages
import 'package:local_auth_darwin/local_auth_darwin.dart' show IOSAuthMessages;

/// Protects mobile settings with the authentication configured on the device.
class ClantoSettingsAuth {
  static final LocalAuthentication _auth = LocalAuthentication();

  static Future<bool> authenticate(String reason) async {
    try {
      // This is false only when the device has neither enrolled biometrics nor
      // a secure device credential, so settings must remain reachable.
      if (!await _auth.isDeviceSupported()) return true;

      return await _auth.authenticate(
        localizedReason: reason,
        authMessages: [
          AndroidAuthMessages(
            signInTitle: translate("Authentication required"),
            biometricHint: translate("Verify identity"),
            biometricNotRecognized: translate("Not recognized. Try again."),
            biometricRequiredTitle: translate("Biometric required"),
            cancelButton: translate("Cancel"),
            deviceCredentialsRequiredTitle:
                translate("Device credentials required"),
            deviceCredentialsSetupDescription:
                translate("Device credentials required"),
            goToSettingsButton: translate("Go to settings"),
            goToSettingsDescription: translate("Biometric required"),
          ),
          IOSAuthMessages(
            lockOut: translate("Not recognized. Try again."),
            goToSettingsButton: translate("Go to settings"),
            goToSettingsDescription: translate("Biometric required"),
            cancelButton: translate("Cancel"),
            localizedFallbackTitle: translate("Device credentials required"),
          ),
        ],
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
