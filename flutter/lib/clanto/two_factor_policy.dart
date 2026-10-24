// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Avviso per `clanto-require-2fa` (src/clanto/two_factor_policy.rs): con la
// policy attiva e il 2FA spento l'host rifiuta ogni connessione in entrata.

import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:settings_ui/settings_ui.dart';

const String kOptClantoRequire2fa = 'clanto-require-2fa';

bool clantoRequire2faUnmet() =>
    bind.mainGetOptionSync(key: kOptClantoRequire2fa) == 'Y' &&
    !bind.mainHasValid2FaSync();

Widget clantoRequire2faNotice() {
  if (!clantoRequire2faUnmet()) return const SizedBox.shrink();
  return Padding(
    padding: const EdgeInsets.only(left: 30, top: 4, bottom: 4),
    child: Text(translate('clanto-require-2fa-tip'),
        style: const TextStyle(color: Colors.red)),
  );
}

SettingsTile clantoRequire2faTile() {
  return SettingsTile(
    title: Text(translate('clanto-require-2fa-tip'),
        style: const TextStyle(color: Colors.red)),
  );
}
