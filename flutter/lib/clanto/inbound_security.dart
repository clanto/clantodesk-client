// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/common/widgets/dialog.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/utils/http_service.dart' as http;
import 'package:settings_ui/settings_ui.dart';

import 'file_transfer_security.dart';
import 'protected_device.dart';
import 'watermark.dart';

const String kOptClantoPostureBypass = 'clanto-posture-bypass';
const String kOptClantoHostWatermark = 'clanto-host-watermark';
const String _postureRulesUrl =
    'https://clanto.it/api/clantodesk-posture.php?rules';

class _PostureRule {
  final String id;
  final String message;
  final bool bypassable;
  final List<String> platforms;

  const _PostureRule({
    required this.id,
    required this.message,
    required this.bypassable,
    required this.platforms,
  });

  factory _PostureRule.fromJson(Map<String, dynamic> json) {
    final platforms = json['platforms'];
    return _PostureRule(
      id: json['id'] as String,
      message: json['message'] as String,
      bypassable: json['bypassable'] as bool,
      platforms: platforms is List
          ? platforms.whereType<String>().toList(growable: false)
          : const [],
    );
  }
}

class ClantoInboundSecuritySetting extends StatefulWidget {
  final bool enabled;

  const ClantoInboundSecuritySetting({Key? key, this.enabled = true})
      : super(key: key);

  @override
  State<ClantoInboundSecuritySetting> createState() =>
      _ClantoInboundSecuritySettingState();
}

class _ClantoInboundSecuritySettingState
    extends State<ClantoInboundSecuritySetting> {
  final Set<String> _bypass = bind
      .mainGetOptionSync(key: kOptClantoPostureBypass)
      .split(',')
      .map((id) => id.trim())
      .where((id) => id.isNotEmpty)
      .toSet();
  List<_PostureRule> _rules = const [];
  String? _error;
  bool _loading = true;
  bool _hasIdWhitelist = idWhitelistNotEmpty();
  bool _requireControllerWatermark = bind.mainGetOptionSync(
          key: kOptClantoRequireControllerWatermark) ==
      'Y';
  bool _hostWatermark =
      bind.mainGetOptionSync(key: kOptClantoHostWatermark) == 'Y';

  @override
  void initState() {
    super.initState();
    _loadRules();
  }

  Future<void> _loadRules() async {
    try {
      final response = await http.get(Uri.parse(_postureRulesUrl));
      if (response.statusCode != 200) throw Exception('HTTP ${response.statusCode}');
      final body = jsonDecode(response.body);
      if (body is! Map<String, dynamic> || body['rules'] is! List) {
        throw const FormatException();
      }
      final rules = (body['rules'] as List)
          .whereType<Map<String, dynamic>>()
          .map(_PostureRule.fromJson)
          .toList(growable: false);
      if (mounted) setState(() {
        _rules = rules;
        _loading = false;
      });
    } catch (_) {
      if (mounted) setState(() {
        _error = translate('Unable to load security rules');
        _loading = false;
      });
    }
  }

  Future<void> _setBypass(_PostureRule rule, bool value) async {
    if (!widget.enabled ||
        isOptionFixed(kOptClantoPostureBypass) ||
        !rule.bypassable) return;
    setState(() {
      if (value) {
        _bypass.add(rule.id);
      } else {
        _bypass.remove(rule.id);
      }
    });
    final bypass = _bypass.toList()..sort();
    await bind.mainSetOption(key: kOptClantoPostureBypass, value: bypass.join(','));
  }

  void _changeIdWhitelist() {
    changeIdWhiteList(callback: () {
      if (mounted) setState(() => _hasIdWhitelist = idWhitelistNotEmpty());
    });
  }

  Future<void> _setRequireControllerWatermark(bool value) async {
    if (!widget.enabled ||
        isOptionFixed(kOptClantoRequireControllerWatermark)) return;
    await bind.mainSetOption(
        key: kOptClantoRequireControllerWatermark,
        value: value ? 'Y' : 'N');
    if (mounted) setState(() => _requireControllerWatermark = value);
  }

  Future<void> _setHostWatermark(bool value) async {
    if (!widget.enabled || isOptionFixed(kOptClantoHostWatermark)) return;
    if (value &&
        isAndroid &&
        !await AndroidPermissionManager.check(kSystemAlertWindow) &&
        !await AndroidPermissionManager.request(kSystemAlertWindow)) {
      return;
    }
    await bind.mainSetOption(
        key: kOptClantoHostWatermark, value: value ? 'Y' : 'N');
    if (mounted) setState(() => _hostWatermark = value);
  }

  @override
  Widget build(BuildContext context) {
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Text(translate('Incoming security'),
          style: Theme.of(context).textTheme.titleMedium),
      if (isMobile)
        CheckboxListTile(
          contentPadding: const EdgeInsets.symmetric(horizontal: 10),
          value: _hasIdWhitelist,
          onChanged: widget.enabled && !isOptionFixed(kOptionIdWhitelist)
              ? (_) => _changeIdWhitelist()
              : null,
          title: Text(translate('Use ID whitelisting')),
          subtitle: Text(translate('id_whitelist_tip')),
        ),
      ClantoProtectedDeviceSetting(enabled: widget.enabled),
      CheckboxListTile(
        contentPadding: const EdgeInsets.symmetric(horizontal: 10),
        value: _requireControllerWatermark,
        onChanged: widget.enabled &&
                !isOptionFixed(kOptClantoRequireControllerWatermark)
            ? (value) => _setRequireControllerWatermark(value ?? false)
            : null,
        title: Text(translate('Require controller watermark')),
        subtitle: Text(translate(
            'Require connecting controllers to show their ID, account user or PC name, and current time.')),
      ),
      CheckboxListTile(
        contentPadding: const EdgeInsets.symmetric(horizontal: 10),
        value: _hostWatermark,
        onChanged: widget.enabled && !isOptionFixed(kOptClantoHostWatermark)
            ? (value) => _setHostWatermark(value ?? false)
            : null,
        title: Text(translate('Show host watermark')),
        subtitle: Text(translate(
            'Show connected controllers, account users, session start time, and active session count on this device.')),
      ),
      ClantoFileTransferSecuritySetting(enabled: widget.enabled),
      if (_loading)
        const Padding(
            padding: EdgeInsets.all(12), child: CircularProgressIndicator()),
      if (_error != null)
        Padding(
            padding: const EdgeInsets.all(10),
            child: Text(_error!, style: const TextStyle(color: Colors.red))),
      ..._rules.map((rule) => CheckboxListTile(
            contentPadding: const EdgeInsets.symmetric(horizontal: 10),
            value: _bypass.contains(rule.id),
            onChanged: widget.enabled &&
                    !isOptionFixed(kOptClantoPostureBypass) &&
                    rule.bypassable
                ? (value) => _setBypass(rule, value ?? false)
                : null,
            title: Text(rule.message),
            subtitle: Text(rule.bypassable
                ? translate('Ignore this rule on this PC')
                : '${translate('Applies to:')} ${rule.platforms.join(', ')}'),
          )),
    ]);
  }
}

SettingsTile clantoInboundSecurityTile() {
  return SettingsTile(
    title: Text(translate('Incoming security')),
    description: Text(translate(
        'Accept connections only from updated and protected devices')),
    onPressed: (context) => showDialog(
      context: context,
      builder: (context) => AlertDialog(
        content: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 480, maxHeight: 600),
          child: SingleChildScrollView(child: ClantoInboundSecuritySetting()),
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
