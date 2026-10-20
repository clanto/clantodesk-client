// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:settings_ui/settings_ui.dart';

const String kOptClantoOutgoingFileTransfer = 'clanto-outgoing-file-transfer';
const String kOptClantoOutgoingClipboard = 'clanto-outgoing-clipboard';
const String kOptClantoOutgoingTunnel = 'clanto-outgoing-tunnel';
const String kOptClantoOutgoingTerminal = 'clanto-outgoing-terminal';
const String kOptClantoOutgoingRemotePrinter =
    'clanto-outgoing-remote-printer';
const String kOptClantoOutgoingRecord = 'clanto-outgoing-record';

const Map<String, String> clantoOutgoingRestrictions = {
  kOptClantoOutgoingFileTransfer: 'Allow file transfer and file copy and paste',
  kOptClantoOutgoingClipboard: 'Allow clipboard sharing',
  kOptClantoOutgoingTunnel: 'Allow port forwarding and RDP',
  kOptClantoOutgoingTerminal: 'Allow terminal',
  kOptClantoOutgoingRemotePrinter: 'Allow remote printer',
  kOptClantoOutgoingRecord: 'Allow session recording',
};

class ClantoOutgoingSecuritySetting extends StatefulWidget {
  final bool enabled;

  const ClantoOutgoingSecuritySetting({Key? key, this.enabled = true})
      : super(key: key);

  @override
  State<ClantoOutgoingSecuritySetting> createState() =>
      _ClantoOutgoingSecuritySettingState();
}

class _ClantoOutgoingSecuritySettingState
    extends State<ClantoOutgoingSecuritySetting> {
  late Map<String, bool> _allowed;

  @override
  void initState() {
    super.initState();
    _allowed = {
      for (final key in clantoOutgoingRestrictions.keys)
        key: bind.mainGetOptionSync(key: key) != 'N',
    };
  }

  Future<void> _setAllowed(String key, bool allowed) async {
    if (!widget.enabled || isOptionFixed(key)) return;
    await bind.mainSetOption(key: key, value: allowed ? 'Y' : 'N');
    if (mounted) setState(() => _allowed[key] = allowed);
  }

  @override
  Widget build(BuildContext context) {
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Text(translate('Outgoing security'),
          style: Theme.of(context).textTheme.titleMedium),
      Text(translate('Choose what this PC may do in sessions it opens.'),
          style: Theme.of(context).textTheme.bodySmall),
      ...clantoOutgoingRestrictions.entries.map((entry) => CheckboxListTile(
            contentPadding: const EdgeInsets.symmetric(horizontal: 10),
            value: _allowed[entry.key],
            onChanged: widget.enabled && !isOptionFixed(entry.key)
                ? (value) => _setAllowed(entry.key, value ?? false)
                : null,
            title: Text(translate(entry.value)),
          )),
    ]);
  }
}

SettingsTile clantoOutgoingSecurityTile() {
  return SettingsTile(
    title: Text(translate('Outgoing security')),
    description: Text(translate('Choose what this PC may do in sessions it opens.')),
    onPressed: (context) => showDialog(
      context: context,
      builder: (context) => AlertDialog(
        content: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 480, maxHeight: 600),
          child: SingleChildScrollView(child: ClantoOutgoingSecuritySetting()),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: Text(translate('Close'))),
        ],
      ),
    ),
  );
}
