// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Punti d'ingresso delle schermate account (dispositivi gestiti, approvazioni,
// collega un PC): visibili solo con account abilitato e login effettuato.

import 'package:flutter/material.dart';
import 'package:flutter_hbb/clanto/access_requests.dart';
import 'package:flutter_hbb/clanto/api.dart';
import 'package:flutter_hbb/clanto/managed_devices.dart';
import 'package:flutter_hbb/clanto/qr_claim.dart';
import 'package:flutter_hbb/common.dart';
import 'package:get/get.dart';
import 'package:settings_ui/settings_ui.dart';

typedef _Entry = ({String key, IconData icon, Future<void> Function(BuildContext) open});

final List<_Entry> _entries = [
  (
    key: 'clanto_managed_devices',
    icon: Icons.devices_other,
    open: showClantoManagedDevices
  ),
  (key: 'clanto_approvals', icon: Icons.verified_user_outlined, open: showClantoApprovals),
  (key: 'clanto_link_pc', icon: Icons.qr_code_2, open: showClantoQrClaim),
];

/// Pulsanti per la scheda Account delle impostazioni desktop.
Widget clantoDesktopAccountButtons() => Obx(() {
      if (!clantoAccountFeaturesAvailable()) return const SizedBox.shrink();
      return Builder(
        builder: (context) => Wrap(spacing: 10, runSpacing: 10, children: [
          for (final e in _entries)
            OutlinedButton.icon(
              icon: Icon(e.icon, size: 18),
              label: Text(translate(e.key)),
              onPressed: () => e.open(context),
            ),
        ]),
      );
    });

/// Voci per la sezione Account delle impostazioni mobile.
AbstractSettingsTile clantoMobileAccountTiles() => CustomSettingsTile(
      child: Builder(
        builder: (context) => Obx(() {
          if (!clantoAccountFeaturesAvailable()) return const SizedBox.shrink();
          return Column(children: [
            for (final e in _entries)
              ListTile(
                leading: Icon(e.icon),
                title: Text(translate(e.key)),
                onTap: () => e.open(context),
              ),
          ]);
        }),
      ),
    );
