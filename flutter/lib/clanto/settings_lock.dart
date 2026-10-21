// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';

const String kOptClantoLockSettings = 'clanto-lock-settings';

bool clantoSettingsLocked() =>
    bind.mainIsOptionFixed(key: kOptClantoLockSettings) &&
    bind.mainGetOptionSync(key: kOptClantoLockSettings) == 'Y';

class ClantoManagedSettingsBanner extends StatelessWidget {
  const ClantoManagedSettingsBanner({Key? key}) : super(key: key);

  @override
  Widget build(BuildContext context) => MaterialBanner(
        content: Text(translate('Settings managed by your organization')),
        leading: const Icon(Icons.business_outlined),
        actions: const [SizedBox.shrink()],
      );
}
