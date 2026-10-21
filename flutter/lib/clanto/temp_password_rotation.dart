// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/models/server_model.dart';
import 'package:settings_ui/settings_ui.dart';

const String kOptClantoTempPasswordRotateMinutes =
    'clanto-temp-password-rotate-minutes';
const List<int> _rotationChoices = [0, 15, 30, 60, 240, 1440];
const int _customChoice = -1;

bool clantoTemporaryPasswordUsed() =>
    bind.mainGetOptionSync(key: kOptionVerificationMethod) !=
    kUsePermanentPassword;

String _rotationLabel(int value) =>
    value == 0 ? translate('Never') : '$value ${translate('minutes')}';

class ClantoTempPasswordRotationSetting extends StatefulWidget {
  const ClantoTempPasswordRotationSetting({Key? key, this.enabled = true})
      : super(key: key);

  final bool enabled;

  @override
  State<ClantoTempPasswordRotationSetting> createState() =>
      _ClantoTempPasswordRotationSettingState();
}

class _ClantoTempPasswordRotationSettingState
    extends State<ClantoTempPasswordRotationSetting> {
  late int _selected;
  final _custom = TextEditingController();
  String? _error;

  bool get _enabled =>
      widget.enabled && !isOptionFixed(kOptClantoTempPasswordRotateMinutes);

  @override
  void initState() {
    super.initState();
    final parsed = int.tryParse(
            bind.mainGetOptionSync(key: kOptClantoTempPasswordRotateMinutes)) ??
        0;
    _selected = _rotationChoices.contains(parsed) ? parsed : _customChoice;
    if (_selected == _customChoice) _custom.text = '$parsed';
  }

  @override
  void dispose() {
    _custom.dispose();
    super.dispose();
  }

  Future<void> _save(int value) async {
    await bind.mainSetOption(
        key: kOptClantoTempPasswordRotateMinutes, value: '$value');
    if (mounted) setState(() => _error = null);
  }

  Future<void> _saveCustom() async {
    final value = int.tryParse(_custom.text.trim());
    if (value == null || value < 1 || value > 1440) {
      setState(() => _error = translate('Value must be between 1 and 1440'));
      return;
    }
    await _save(value);
  }

  @override
  Widget build(BuildContext context) => Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Wrap(
            spacing: 12,
            runSpacing: 8,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              Text(translate('Rotate one-time password')),
              SizedBox(
                width: 190,
                child: DropdownButton<int>(
                  isExpanded: true,
                  value: _selected,
                  onChanged: _enabled
                      ? (value) async {
                          if (value == null) return;
                          setState(() {
                            _selected = value;
                            _error = null;
                          });
                          if (value != _customChoice) await _save(value);
                        }
                      : null,
                  items: [
                    ..._rotationChoices.map((value) => DropdownMenuItem(
                          value: value,
                          child: Text(_rotationLabel(value)),
                        )),
                    DropdownMenuItem(
                      value: _customChoice,
                      child: Text(translate('Custom')),
                    ),
                  ],
                ),
              ),
            ],
          ),
          if (_selected == _customChoice)
            Wrap(
              spacing: 8,
              runSpacing: 8,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                SizedBox(
                  width: 220,
                  child: TextField(
                    controller: _custom,
                    enabled: _enabled,
                    keyboardType: TextInputType.number,
                    inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                    decoration: InputDecoration(
                      isDense: true,
                      labelText: translate('Custom interval (minutes)'),
                      errorText: _error,
                    ),
                    onSubmitted: (_) => _saveCustom(),
                  ).workaroundFreezeLinuxMint(),
                ),
                ElevatedButton(
                  onPressed: _enabled ? _saveCustom : null,
                  child: Text(translate('Apply')),
                ),
              ],
            ),
        ],
      );
}

SettingsTile clantoTempPasswordRotationTile(
    {bool enabled = true, VoidCallback? refresh}) {
  final raw = int.tryParse(
          bind.mainGetOptionSync(key: kOptClantoTempPasswordRotateMinutes)) ??
      0;
  final canChange =
      enabled && !isOptionFixed(kOptClantoTempPasswordRotateMinutes);
  return SettingsTile(
    title: Text(translate('Rotate one-time password')),
    value: Text(_rotationLabel(raw)),
    leading: const Icon(Icons.autorenew),
    onPressed: canChange
        ? (context) async {
            await showDialog<void>(
              context: context,
              builder: (context) => AlertDialog(
                title: Text(translate('Rotate one-time password')),
                content: SizedBox(
                  width: 430,
                  child: ClantoTempPasswordRotationSetting(enabled: enabled),
                ),
                actions: [
                  TextButton(
                    onPressed: () => Navigator.of(context).pop(),
                    child: Text(translate('Close')),
                  ),
                ],
              ),
            );
            refresh?.call();
          }
        : null,
  );
}
