// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'dart:convert';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_hbb/clanto/mobile_file_transfer.dart';
import 'package:flutter_hbb/common.dart';
import 'package:settings_ui/settings_ui.dart';

const String kOptClantoSessionLogDays = 'clanto-session-log-days';

enum _ReportPeriod { today, week, month, all }

class _SessionRecord {
  const _SessionRecord({
    required this.startedAt,
    required this.endedAt,
    required this.durationSecs,
    required this.sessionCode,
    required this.peerId,
    required this.peerName,
    required this.ip,
    required this.connectionType,
    required this.primaryAuth,
    required this.twoFactor,
    required this.postureResult,
    required this.postureBypasses,
    required this.permissions,
    required this.files,
    required this.closeReason,
    required this.rejected,
    required this.closed,
  });

  final DateTime? startedAt;
  final DateTime? endedAt;
  final int durationSecs;
  final String sessionCode;
  final String peerId;
  final String peerName;
  final String ip;
  final String connectionType;
  final String primaryAuth;
  final String twoFactor;
  final String postureResult;
  final List<String> postureBypasses;
  final Set<String> permissions;
  final List<Map<String, dynamic>> files;
  final String closeReason;
  final bool rejected;
  final bool closed;

  factory _SessionRecord.fromJson(Map<String, dynamic> json) {
    final rawPermissions = json['permissions_used'] ?? json['permissions'];
    final permissions = <String>{};
    if (rawPermissions is Map) {
      rawPermissions.forEach((key, value) {
        if (value == true || value == 'Y') permissions.add(key.toString());
      });
    } else if (rawPermissions is List) {
      permissions.addAll(rawPermissions.map((value) => value.toString()));
    }
    final rawFiles = json['files'];
    return _SessionRecord(
      startedAt:
          _date(json['started_at'] ?? json['occurred_at'] ?? json['start']),
      endedAt: _date(json['ended_at'] ?? json['end']),
      durationSecs: _integer(json['duration_secs']),
      sessionCode: _string(json['session_code']),
      peerId: _string(json['peer_id']),
      peerName: _string(json['peer_name']),
      ip: _string(json['ip']),
      connectionType: _string(json['connection_type'] ?? json['type']),
      primaryAuth: _primaryAuth(json['primary_auth']),
      twoFactor: _twoFactor(json['two_factor']),
      postureResult: _string(json['posture_result']),
      postureBypasses: (json['posture_bypasses'] as List?)
              ?.map((value) => value.toString())
              .toList(growable: false) ??
          const [],
      permissions: permissions,
      files: rawFiles is List
          ? rawFiles.whereType<Map>().map((file) {
              return file.map((key, value) => MapEntry(key.toString(), value));
            }).toList(growable: false)
          : const [],
      closeReason:
          _string(json['close_reason'] ?? json['alarm_type'] ?? json['details']),
      rejected: json['record_type'] == 'rejected_attempt',
      closed: json['closed'] == true,
    );
  }

  static String _string(dynamic value) => value?.toString() ?? '';

  static int _integer(dynamic value) =>
      value is num ? value.toInt() : int.tryParse(_string(value)) ?? 0;

  static String _primaryAuth(dynamic value) {
    switch (_integer(value)) {
      case 1:
        return translate('Approval');
      case 2:
        return translate('Temporary password');
      case 3:
        return translate('Permanent password');
      case 4:
        return translate('Switch sides');
      default:
        return '';
    }
  }

  static String _twoFactor(dynamic value) {
    switch (_integer(value)) {
      case 1:
        return 'TOTP';
      case 2:
        return translate('Trusted device');
      default:
        return '';
    }
  }

  static DateTime? _date(dynamic value) {
    if (value is num) {
      return DateTime.fromMillisecondsSinceEpoch(value.toInt(), isUtc: true)
          .toLocal();
    }
    final text = _string(value);
    if (text.isEmpty) return null;
    final number = int.tryParse(text);
    if (number != null) {
      return DateTime.fromMillisecondsSinceEpoch(number, isUtc: true).toLocal();
    }
    return DateTime.tryParse(text)?.toLocal();
  }

  String get peer => peerName.isEmpty ? peerId : '$peerName ($peerId)';

  String get accessMethod {
    final methods = [primaryAuth, if (twoFactor.isNotEmpty) twoFactor]
        .where((method) => method.isNotEmpty);
    return methods.isEmpty ? '-' : methods.join(' + ');
  }

  String statusLabel() =>
      closed ? translate('Ended') : translate('Not closed');
}

class ClantoSessionReport extends StatefulWidget {
  const ClantoSessionReport({Key? key, this.enabled = true}) : super(key: key);

  final bool enabled;

  @override
  State<ClantoSessionReport> createState() => _ClantoSessionReportState();
}

class _ClantoSessionReportState extends State<ClantoSessionReport> {
  _ReportPeriod _period = _ReportPeriod.week;
  List<_SessionRecord> _records = const [];
  String? _error;
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  int _fromMs() {
    final now = DateTime.now();
    switch (_period) {
      case _ReportPeriod.today:
        return DateTime(now.year, now.month, now.day).millisecondsSinceEpoch;
      case _ReportPeriod.week:
        return now.subtract(const Duration(days: 7)).millisecondsSinceEpoch;
      case _ReportPeriod.month:
        return now.subtract(const Duration(days: 30)).millisecondsSinceEpoch;
      case _ReportPeriod.all:
        return 0;
    }
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final raw = await bind.mainGetSessionReport(
          fromMs: _fromMs(), toMs: DateTime.now().millisecondsSinceEpoch);
      final decoded = jsonDecode(raw);
      final List<dynamic> values = decoded is List
          ? decoded
          : decoded is Map && decoded['records'] is List
              ? decoded['records'] as List<dynamic>
              : const [];
      final records = values
          .whereType<Map>()
          .map((value) => _SessionRecord.fromJson(
              value.map((key, item) => MapEntry(key.toString(), item))))
          .toList(growable: false);
      if (mounted) setState(() => _records = records);
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  String _periodLabel(_ReportPeriod period) {
    switch (period) {
      case _ReportPeriod.today:
        return translate('Today');
      case _ReportPeriod.week:
        return translate('Last 7 days');
      case _ReportPeriod.month:
        return translate('Last 30 days');
      case _ReportPeriod.all:
        return translate('All time');
    }
  }

  String _duration(int seconds) {
    final duration = Duration(seconds: seconds);
    final hours = duration.inHours;
    final minutes = duration.inMinutes.remainder(60);
    final secs = duration.inSeconds.remainder(60);
    if (hours > 0) return '${hours}h ${minutes}m';
    if (minutes > 0) return '${minutes}m ${secs}s';
    return '${secs}s';
  }

  String _date(DateTime? value) {
    if (value == null) return '-';
    return '${value.year.toString().padLeft(4, '0')}-'
        '${value.month.toString().padLeft(2, '0')}-'
        '${value.day.toString().padLeft(2, '0')} '
        '${value.hour.toString().padLeft(2, '0')}:'
        '${value.minute.toString().padLeft(2, '0')}';
  }

  List<Widget> _permissionIcons(_SessionRecord record) {
    const icons = <String, IconData>{
      'input': Icons.keyboard,
      'keyboard_mouse': Icons.keyboard,
      'clipboard': Icons.content_paste,
      'file_transfer': Icons.folder,
      'audio': Icons.volume_up,
      'recording': Icons.fiber_manual_record,
      'remote_restart': Icons.restart_alt,
      'terminal': Icons.terminal,
      'tunnel': Icons.route,
    };
    return record.permissions
        .map((permission) => icons[permission])
        .whereType<IconData>()
        .map((icon) => Padding(
              padding: const EdgeInsets.only(right: 4),
              child: Icon(icon, size: 17),
            ))
        .toList(growable: false);
  }

  String _csv() {
    String quote(String value) => '"${value.replaceAll('"', '""')}"';
    final rows = <List<String>>[
      [
        'Started at',
        'Ended at',
        'Ended',
        'Duration seconds',
        'Session code',
        'Peer ID',
        'Peer name',
        'IP',
        'Connection type',
        'Access method',
        'Posture result',
        'Bypassed rules',
        'Permissions used',
        'File transfers',
        'Close reason',
        'Rejected',
      ],
      ..._records.map((record) => [
            _date(record.startedAt),
            _date(record.endedAt),
            record.statusLabel(),
            record.closed ? '${record.durationSecs}' : '',
            record.sessionCode,
            record.peerId,
            record.peerName,
            record.ip,
            record.connectionType,
            record.accessMethod,
            record.postureResult,
            record.postureBypasses.join(', '),
            record.permissions.join(', '),
            record.files
                .map((file) => [file['direction'], file['name'], file['size']]
                    .where((value) => value != null)
                    .join(' · '))
                .join('; '),
            record.closeReason,
            record.rejected ? 'yes' : 'no',
          ]),
    ];
    return '${rows.map((row) => row.map(quote).join(',')).join('\r\n')}\r\n';
  }

  Future<void> _exportCsv() async {
    if (!widget.enabled) return;
    final name = 'clantodesk-session-report-'
        '${DateTime.now().toIso8601String().substring(0, 10)}.csv';
    final csv = _csv();
    try {
      if (isMobile) {
        final file = File('${Directory.systemTemp.path}${Platform.pathSeparator}$name');
        await file.writeAsString(csv, flush: true);
        await MobileFileTransfer.publishReceivedFile(file.path, name);
      } else {
        final target = await FilePicker.platform.saveFile(
          dialogTitle: '${translate('Export CSV')}...',
          fileName: name,
          type: FileType.custom,
          allowedExtensions: const ['csv'],
        );
        if (target == null) return;
        await File(target).writeAsString(csv, flush: true);
      }
      if (mounted) showToast(translate('Successful'));
    } catch (error) {
      debugPrint('Unable to export session report: $error');
      if (mounted) showToast(translate('Failed'));
    }
  }

  void _showDetail(_SessionRecord record) {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(record.rejected
            ? translate('Rejected connection attempt')
            : translate('Session details')),
        content: SingleChildScrollView(
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            _detail('Date', _date(record.startedAt)),
            _detail('Ended',
                record.closed ? _date(record.endedAt) : record.statusLabel()),
            if (record.closed) _detail('Duration', _duration(record.durationSecs)),
            _detail('Session code', record.sessionCode),
            _detail('Peer', record.peer),
            _detail('IP', record.ip),
            _detail('Connection type', record.connectionType),
            _detail('Access method', record.accessMethod),
            _detail('Posture result', record.postureResult),
            if (record.postureBypasses.isNotEmpty)
              _detail('Bypassed rules', record.postureBypasses.join(', ')),
            _detail('Permissions used', record.permissions.join(', ')),
            _detail('File transfers', '${record.files.length}'),
            if (record.closeReason.isNotEmpty)
              _detail('Close reason', record.closeReason),
            if (record.files.isNotEmpty) ...[
              const SizedBox(height: 12),
              Text(translate('File transfers'),
                  style: Theme.of(context).textTheme.titleSmall),
              ...record.files.map((file) => Text([
                    file['direction'],
                    file['name'],
                    file['size'],
                  ].where((value) => value != null).join(' · '))),
            ],
          ]),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: Text(translate('Close'))),
        ],
      ),
    );
  }

  Widget _detail(String label, String value) => Padding(
        padding: const EdgeInsets.only(bottom: 8),
        child: Text('${translate(label)}: ${value.isEmpty ? '-' : value}'),
      );

  @override
  Widget build(BuildContext context) {
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Row(children: [
        Expanded(
          child: DropdownButton<_ReportPeriod>(
            isExpanded: true,
            value: _period,
            items: _ReportPeriod.values
                .map((period) => DropdownMenuItem(
                    value: period, child: Text(_periodLabel(period))))
                .toList(growable: false),
            onChanged: (period) {
              if (period == null) return;
              setState(() => _period = period);
              _load();
            },
          ),
        ),
        IconButton(
            tooltip: translate('Refresh'), icon: const Icon(Icons.refresh), onPressed: _load),
        TextButton.icon(
          onPressed: widget.enabled && !_loading ? _exportCsv : null,
          icon: const Icon(Icons.file_download_outlined),
          label: Text(translate('Export CSV')),
        ),
      ]),
      if (_loading)
        const Padding(
            padding: EdgeInsets.all(20), child: Center(child: CircularProgressIndicator()))
      else if (_error != null)
        Padding(
          padding: const EdgeInsets.all(12),
          child: Text(_error!, style: const TextStyle(color: Colors.red)),
        )
      else if (_records.isEmpty)
        Padding(
          padding: const EdgeInsets.all(18),
          child: Text(translate('No sessions found')),
        )
      else
        ..._records.map((record) => ListTile(
              dense: true,
              leading: Icon(record.rejected ? Icons.block : Icons.computer),
              title: Text(record.rejected
                  ? translate('Rejected connection attempt')
                  : record.peer),
              subtitle: Text('${_date(record.startedAt)} · '
                  '${record.connectionType.isEmpty ? '-' : record.connectionType} · '
                  '${record.accessMethod} · ${record.statusLabel()}'),
              trailing: SizedBox(
                width: 170,
                child: Wrap(
                  alignment: WrapAlignment.end,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  spacing: 4,
                  children: [
                    ..._permissionIcons(record),
                    const Icon(Icons.folder, size: 17),
                    Text('${record.files.length}',
                        style: Theme.of(context).textTheme.bodySmall),
                    Text(record.closed
                        ? _duration(record.durationSecs)
                        : record.statusLabel(),
                        style: Theme.of(context).textTheme.bodySmall),
                  ],
                ),
              ),
              onTap: () => _showDetail(record),
            )),
    ]);
  }
}

SettingsTile clantoSessionReportTile() {
  return SettingsTile(
    title: Text(translate('Report sessions')),
    leading: const Icon(Icons.assessment_outlined),
    onPressed: (context) => showDialog(
      context: context,
      builder: (context) => AlertDialog(
        content: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 680, maxHeight: 650),
          child: SingleChildScrollView(
              child: ClantoSessionReport(
                  enabled: !isOptionFixed(kOptClantoSessionLogDays))),
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
