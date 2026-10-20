// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:get/get.dart';

const String kOptClantoFileScan = 'clanto-file-scan';
const String kOptClantoFileScanClamd = 'clanto-file-scan-clamd';
const String kOptClantoFileScanMaxMb = 'clanto-file-scan-max-mb';
const String kOptClantoFileDirection = 'clanto-file-direction';
const String kOptClantoFileBlockedExt = 'clanto-file-blocked-ext';

const Set<String> _scanModes = {'off', 'warn', 'block'};
const Set<String> _directions = {'both', 'to-host', 'from-host', 'none'};

class ClantoFileTransferSecuritySetting extends StatefulWidget {
  final bool enabled;

  const ClantoFileTransferSecuritySetting({Key? key, this.enabled = true})
      : super(key: key);

  @override
  State<ClantoFileTransferSecuritySetting> createState() =>
      _ClantoFileTransferSecuritySettingState();
}

class _ClantoFileTransferSecuritySettingState
    extends State<ClantoFileTransferSecuritySetting> {
  late String _scanMode;
  late String _direction;
  final _clamdEndpoint = TextEditingController();
  final _maxMb = TextEditingController();
  final _blockedExtensions = TextEditingController();

  @override
  void initState() {
    super.initState();
    final scanMode = bind.mainGetOptionSync(key: kOptClantoFileScan);
    _scanMode = _scanModes.contains(scanMode) ? scanMode : 'off';
    final direction = bind.mainGetOptionSync(key: kOptClantoFileDirection);
    _direction = _directions.contains(direction) ? direction : 'both';
    _clamdEndpoint.text =
        bind.mainGetOptionSync(key: kOptClantoFileScanClamd);
    final maxMb = bind.mainGetOptionSync(key: kOptClantoFileScanMaxMb);
    _maxMb.text = maxMb.isEmpty ? '512' : maxMb;
    _blockedExtensions.text =
        bind.mainGetOptionSync(key: kOptClantoFileBlockedExt);
  }

  @override
  void dispose() {
    _clamdEndpoint.dispose();
    _maxMb.dispose();
    _blockedExtensions.dispose();
    super.dispose();
  }

  Future<void> _setScanMode(String? value) async {
    if (value == null ||
        !widget.enabled ||
        isOptionFixed(kOptClantoFileScan)) {
      return;
    }
    await bind.mainSetOption(key: kOptClantoFileScan, value: value);
    if (mounted) setState(() => _scanMode = value);
  }

  Future<void> _setScanEnabled(bool enabled) async {
    await _setScanMode(enabled ? 'block' : 'off');
  }

  Future<void> _setDirection(String? value) async {
    if (value == null ||
        !widget.enabled ||
        isOptionFixed(kOptClantoFileDirection)) {
      return;
    }
    await bind.mainSetOption(key: kOptClantoFileDirection, value: value);
    if (mounted) setState(() => _direction = value);
  }

  Future<void> _setTextOption(String key, String value) async {
    if (!widget.enabled || isOptionFixed(key)) return;
    await bind.mainSetOption(key: key, value: value.trim());
  }

  Future<void> _setMaxMb() async {
    if (!widget.enabled || isOptionFixed(kOptClantoFileScanMaxMb)) return;
    final value = int.tryParse(_maxMb.text.trim());
    final normalized = value != null && value > 0 ? '$value' : '512';
    if (_maxMb.text != normalized) {
      _maxMb.text = normalized;
    }
    await bind.mainSetOption(
        key: kOptClantoFileScanMaxMb, value: normalized);
  }

  Widget _dropdown(String label, String value,
      List<DropdownMenuItem<String>> items, ValueChanged<String?>? onChanged) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(10, 8, 10, 0),
      child: Row(children: [
        Expanded(child: Text(translate(label))),
        const SizedBox(width: 12),
        Expanded(
          child: DropdownButton<String>(
            isExpanded: true,
            value: value,
            onChanged: onChanged,
            items: items,
          ),
        ),
      ]),
    );
  }

  Widget _field(
      String label, String? helper, TextEditingController controller,
      {required String optionKey,
      bool numeric = false,
      required VoidCallback onDone}) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(10, 8, 10, 0),
      child: TextField(
        controller: controller,
        enabled: widget.enabled && !isOptionFixed(optionKey),
        keyboardType: numeric ? TextInputType.number : TextInputType.text,
        inputFormatters:
            numeric ? [FilteringTextInputFormatter.digitsOnly] : null,
        decoration: InputDecoration(
          isDense: true,
          labelText: translate(label),
          helperText: helper == null ? null : translate(helper),
        ),
        onSubmitted: (_) => onDone(),
        onTapOutside: (_) => onDone(),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final scanEnabled = _scanMode != 'off';
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      const Divider().marginSymmetric(horizontal: 10),
      CheckboxListTile(
        contentPadding: const EdgeInsets.symmetric(horizontal: 10),
        value: scanEnabled,
        onChanged: widget.enabled && !isOptionFixed(kOptClantoFileScan)
            ? (value) => _setScanEnabled(value == true)
            : null,
        title: Text(translate('Scan received files for malware')),
      ),
      if (scanEnabled) ...[
        _dropdown(
          'Scan mode',
          _scanMode,
          [
            DropdownMenuItem(
                value: 'warn', child: Text(translate('Warn and deliver'))),
            DropdownMenuItem(
                value: 'block', child: Text(translate('Block unsafe files'))),
          ],
          widget.enabled && !isOptionFixed(kOptClantoFileScan)
              ? _setScanMode
              : null,
        ),
        if (!isWindows)
          _field(
            'ClamAV endpoint',
            'Unix socket path or host:port',
            _clamdEndpoint,
            optionKey: kOptClantoFileScanClamd,
            onDone: () => _setTextOption(
                kOptClantoFileScanClamd, _clamdEndpoint.text),
          ),
        _field(
          'Maximum file size (MB)',
          null,
          _maxMb,
          optionKey: kOptClantoFileScanMaxMb,
          numeric: true,
          onDone: _setMaxMb,
        ),
      ],
      _dropdown(
        'File transfer direction',
        _direction,
        [
          DropdownMenuItem(
              value: 'both', child: Text(translate('Both directions'))),
          DropdownMenuItem(
              value: 'to-host',
              child: Text(translate('Only to this device'))),
          DropdownMenuItem(
              value: 'from-host',
              child: Text(translate('Only from this device'))),
          DropdownMenuItem(
              value: 'none', child: Text(translate('No file transfers'))),
        ],
        widget.enabled && !isOptionFixed(kOptClantoFileDirection)
            ? _setDirection
            : null,
      ),
      _field(
        'Blocked file extensions',
        'Comma-separated extensions without dots',
        _blockedExtensions,
        optionKey: kOptClantoFileBlockedExt,
        onDone: () => _setTextOption(
            kOptClantoFileBlockedExt, _blockedExtensions.text),
      ),
    ]);
  }
}
