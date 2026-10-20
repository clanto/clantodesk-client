// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/utils/http_service.dart' as http;
import 'package:settings_ui/settings_ui.dart';

import 'file_transfer_security.dart';
import 'id_permissions.dart';
import 'protected_device.dart';
import 'watermark.dart';

const String kOptClantoPostureBypass = 'clanto-posture-bypass';
const String kOptClantoHostWatermark = 'clanto-host-watermark';
const String kOptClantoControllerWatermark =
    kOptClantoRequireControllerWatermark;
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

Future<void> showClantoPostureRulesDialog(BuildContext context,
    {bool enabled = true}) async {
  await showDialog<void>(
    context: context,
    builder: (context) => _PostureRulesDialog(enabled: enabled),
  );
}

class _PostureRulesDialog extends StatefulWidget {
  const _PostureRulesDialog({required this.enabled});

  final bool enabled;

  @override
  State<_PostureRulesDialog> createState() => _PostureRulesDialogState();
}

class _PostureRulesDialogState extends State<_PostureRulesDialog> {
  final Set<String> _bypass = bind
      .mainGetOptionSync(key: kOptClantoPostureBypass)
      .split(',')
      .map((id) => id.trim())
      .where((id) => id.isNotEmpty)
      .toSet();
  List<_PostureRule> _rules = const [];
  bool _loading = true;
  bool _loadFailed = false;

  @override
  void initState() {
    super.initState();
    _loadRules();
  }

  Future<void> _loadRules() async {
    try {
      final response = await http.get(Uri.parse(_postureRulesUrl));
      if (response.statusCode != 200) {
        throw Exception('HTTP ${response.statusCode}');
      }
      final body = jsonDecode(response.body);
      if (body is! Map<String, dynamic> || body['rules'] is! List) {
        throw const FormatException();
      }
      final rules = (body['rules'] as List)
          .whereType<Map<String, dynamic>>()
          .map(_PostureRule.fromJson)
          .toList(growable: false);
      if (mounted) {
        setState(() {
          _rules = rules;
          _loading = false;
        });
      }
    } catch (error) {
      debugPrint('Unable to load security rules: $error');
      if (mounted) {
        setState(() {
          _loadFailed = true;
          _loading = false;
        });
      }
    }
  }

  Future<void> _setBypass(_PostureRule rule, bool value) async {
    if (!widget.enabled ||
        isOptionFixed(kOptClantoPostureBypass) ||
        !rule.bypassable) {
      return;
    }
    setState(() {
      if (value) {
        _bypass.add(rule.id);
      } else {
        _bypass.remove(rule.id);
      }
    });
    final bypass = _bypass.toList()..sort();
    await bind.mainSetOption(
        key: kOptClantoPostureBypass, value: bypass.join(','));
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(translate('Device posture rules')),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520, maxHeight: 600),
        child: _loading
            ? const Center(child: CircularProgressIndicator())
            : _loadFailed
                ? Text(translate('Unable to load security rules'),
                    style: const TextStyle(color: Colors.red))
                : SingleChildScrollView(
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: _rules
                          .map((rule) => CheckboxListTile(
                                contentPadding: EdgeInsets.zero,
                                value: _bypass.contains(rule.id),
                                onChanged: widget.enabled &&
                                        !isOptionFixed(
                                            kOptClantoPostureBypass) &&
                                        rule.bypassable
                                    ? (value) =>
                                        _setBypass(rule, value ?? false)
                                    : null,
                                title: Text(rule.message),
                                subtitle: Text(rule.bypassable
                                    ? translate('Ignore this rule on this PC')
                                    : '${translate('Applies to:')} ${rule.platforms.join(', ')}'),
                              ))
                          .toList(growable: false),
                    ),
                  ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(translate('Close')),
        ),
      ],
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
  bool _hasIdWhitelist = idWhitelistNotEmpty();
  bool _requireControllerWatermark = bind.mainGetOptionSync(
          key: kOptClantoRequireControllerWatermark) ==
      'Y';
  bool _hostWatermark =
      bind.mainGetOptionSync(key: kOptClantoHostWatermark) == 'Y';

  Future<void> _changeIdWhitelist() async {
    final saved = await showClantoIdPermissionsDialog(
      context,
      enabled: widget.enabled,
    );
    if (saved && mounted) {
      setState(() => _hasIdWhitelist = idWhitelistNotEmpty());
    }
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
        InkWell(
          onTap: _changeIdWhitelist,
          child: IgnorePointer(
            child: CheckboxListTile(
              contentPadding: const EdgeInsets.symmetric(horizontal: 10),
              value: _hasIdWhitelist,
              onChanged: widget.enabled &&
                      !isOptionFixed(kOptionIdWhitelist) &&
                      !isOptionFixed(kOptClantoIdPermissions)
                  ? (_) {}
                  : null,
              title: Text(translate('Use ID whitelisting')),
              subtitle: Text(translate('id_whitelist_tip')),
            ),
          ),
        ),
      ClantoProtectedDeviceSetting(
        enabled: widget.enabled,
        onModify: () => showClantoPostureRulesDialog(context,
            enabled: widget.enabled),
      ),
      CheckboxListTile(
        contentPadding: const EdgeInsets.symmetric(horizontal: 10),
        value: _requireControllerWatermark,
        onChanged: widget.enabled &&
                !isOptionFixed(kOptClantoRequireControllerWatermark)
            ? (value) => _setRequireControllerWatermark(value ?? false)
            : null,
        title: Text(translate('Require controller watermark')),
        subtitle: Text(translate(
            'Require connecting controllers to show controller identity, host identity, time, and session code.')),
      ),
      CheckboxListTile(
        contentPadding: const EdgeInsets.symmetric(horizontal: 10),
        value: _hostWatermark,
        onChanged: widget.enabled && !isOptionFixed(kOptClantoHostWatermark)
            ? (value) => _setHostWatermark(value ?? false)
            : null,
        title: Text(translate('Show host watermark')),
        subtitle: Text(translate(
            'Show a bar with the technician, session code and start time on this device.')),
      ),
      ClantoFileTransferSecuritySetting(enabled: widget.enabled),
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
