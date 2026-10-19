// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:get/get.dart';
import 'package:settings_ui/settings_ui.dart';

const String kOptClantoRequireProtectedDevice =
    'clanto-require-protected-device';

bool clantoRequireProtectedDevice() =>
    bind.mainGetOptionSync(key: kOptClantoRequireProtectedDevice) == 'Y';

Future<void> _setClantoRequireProtectedDevice(bool on) => bind.mainSetOption(
    key: kOptClantoRequireProtectedDevice, value: on ? 'Y' : 'N');

class ClantoProtectedDeviceSetting extends StatefulWidget {
  final bool enabled;

  const ClantoProtectedDeviceSetting({Key? key, this.enabled = true})
      : super(key: key);

  @override
  State<ClantoProtectedDeviceSetting> createState() =>
      _ClantoProtectedDeviceSettingState();
}

class _ClantoProtectedDeviceSettingState
    extends State<ClantoProtectedDeviceSetting> {
  bool _on = clantoRequireProtectedDevice();

  Future<void> _toggle() async {
    if (!widget.enabled) return;
    await _setClantoRequireProtectedDevice(!_on);
    setState(() => _on = clantoRequireProtectedDevice());
  }

  @override
  Widget build(BuildContext context) {
    final enabled = widget.enabled && !isOptionFixed(kOptClantoRequireProtectedDevice);
    return GestureDetector(
      onTap: enabled ? _toggle : null,
      child: Row(children: [
        Checkbox(
                value: _on,
                onChanged: enabled ? (_) => _toggle() : null)
            .marginOnly(right: 5),
        Expanded(
            child:
                Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text(translate(
              'Accept connections only from updated and protected devices')),
          Text(
              translate(
                  'Requires an Internet connection to clanto.it to verify the connecting device.'),
              style: Theme.of(context).textTheme.bodySmall),
        ])),
      ]).marginOnly(left: 10, top: 5, bottom: 5),
    );
  }
}

SettingsTile clantoProtectedDeviceTile(VoidCallback refresh) {
  return SettingsTile.switchTile(
    initialValue: clantoRequireProtectedDevice(),
    title: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Text(translate(
          'Accept connections only from updated and protected devices')),
      Text(translate(
          'Requires an Internet connection to clanto.it to verify the connecting device.')),
    ]),
    onToggle: isOptionFixed(kOptClantoRequireProtectedDevice)
        ? null
        : (on) async {
      await _setClantoRequireProtectedDevice(on);
      refresh();
    },
  );
}
