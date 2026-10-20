// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_hbb/clanto/mobile_file_transfer.dart';
import 'package:flutter_hbb/clanto/session_report_data.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:settings_ui/settings_ui.dart';

const String kOptClantoSessionLogDays = 'clanto-session-log-days';

enum _ReportPeriod { today, week, month, all }

class ClantoSessionReport extends StatefulWidget {
  const ClantoSessionReport({Key? key, this.enabled = true}) : super(key: key);

  final bool enabled;

  @override
  State<ClantoSessionReport> createState() => _ClantoSessionReportState();
}

class _ClantoSessionReportState extends State<ClantoSessionReport> {
  _ReportPeriod _period = _ReportPeriod.week;
  ClantoSessionOutcomeFilter _outcome = ClantoSessionOutcomeFilter.all;
  String _connectionType = '';
  final _search = TextEditingController();
  List<ClantoSessionRecord> _records = const [];
  String? _error;
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
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
      final records = clantoParseSessionRecords(raw);
      if (mounted) {
        setState(() {
          _records = records;
          if (_connectionType.isNotEmpty &&
              !records.any(
                  (record) => record.connectionType == _connectionType)) {
            _connectionType = '';
          }
        });
      }
    } catch (error) {
      debugPrint('Unable to load session report: $error');
      if (mounted) {
        setState(() => _error = translate('Unable to load session report'));
      }
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

  List<Widget> _permissionIcons(ClantoSessionRecord record) {
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

  List<ClantoSessionRecord> get _filteredRecords =>
      clantoFilterSessionRecords(
        _records,
        outcome: _outcome,
        connectionType: _connectionType,
        query: _search.text,
      );

  Future<void> _exportCsv() async {
    if (!widget.enabled) return;
    final name = 'clantodesk-session-report-'
        '${DateTime.now().toIso8601String().substring(0, 10)}.csv';
    final csv = clantoSessionRecordsCsv(
      _filteredRecords,
      formatDate: _date,
      translate: translate,
    );
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

  String _outcomeFilterLabel(ClantoSessionOutcomeFilter outcome) {
    switch (outcome) {
      case ClantoSessionOutcomeFilter.all:
        return translate('All outcomes');
      case ClantoSessionOutcomeFilter.successful:
        return translate('Successful sessions');
      case ClantoSessionOutcomeFilter.rejected:
        return translate('Rejected sessions');
      case ClantoSessionOutcomeFilter.inProgress:
        return translate('Sessions in progress');
    }
  }

  String _idProfileLabel(String profile) {
    return switch (profile) {
      'view' => translate('View only'),
      'support' => translate('Support'),
      'full' => translate('Full control'),
      'custom' => translate('Custom'),
      _ => profile.isEmpty ? '-' : profile,
    };
  }

  String _outcomeLabel(ClantoSessionRecord record) {
    switch (record.outcome) {
      case ClantoSessionOutcome.successful:
        return translate('Session successful');
      case ClantoSessionOutcome.rejected:
        return translate('Session rejected');
      case ClantoSessionOutcome.inProgress:
        return translate('Session in progress');
    }
  }

  Color _outcomeColor(ClantoSessionRecord record) {
    switch (record.outcome) {
      case ClantoSessionOutcome.successful:
        return Colors.green;
      case ClantoSessionOutcome.rejected:
        return Colors.red;
      case ClantoSessionOutcome.inProgress:
        return Colors.orange;
    }
  }

  Widget _outcomeBadge(ClantoSessionRecord record) {
    final color = _outcomeColor(record);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      decoration: BoxDecoration(
        color: color.withOpacity(0.14),
        borderRadius: BorderRadius.circular(12),
      ),
      child: Text(_outcomeLabel(record),
          style: TextStyle(color: color, fontWeight: FontWeight.w600)),
    );
  }

  void _showDetail(ClantoSessionRecord record) {
    showDialog(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(record.rejected
            ? translate('Rejected connection attempt')
            : translate('Session details')),
        content: SingleChildScrollView(
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            _detail('Date', _date(record.startedAt)),
            _detail('Ended', record.closed
                ? _date(record.endedAt)
                : record.statusLabel(translate)),
            if (record.closed) _detail('Duration', _duration(record.durationSecs)),
            _detail('Session code', record.sessionCode),
            _detail('Peer', record.peer),
            _detail('IP', record.ip),
            _detail('Connection type', record.connectionType),
            _detail('ID profile', _idProfileLabel(record.idProfile)),
            _detail('Access method', record.accessMethod(translate)),
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

  Widget _filters(BuildContext context) {
    final connectionTypes = _records
        .map((record) => record.connectionType)
        .where((type) => type.isNotEmpty)
        .toSet()
        .toList()
      ..sort();
    final selectedType = connectionTypes.contains(_connectionType)
        ? _connectionType
        : '';
    final searchWidth = isMobile
        ? (MediaQuery.sizeOf(context).width - 128)
            .clamp(160.0, 280.0)
            .toDouble()
        : 220.0;
    return Wrap(
      spacing: 10,
      runSpacing: 8,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        SizedBox(
          width: 145,
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
        SizedBox(
          width: 170,
          child: DropdownButton<ClantoSessionOutcomeFilter>(
            isExpanded: true,
            value: _outcome,
            items: ClantoSessionOutcomeFilter.values
                .map((outcome) => DropdownMenuItem(
                    value: outcome,
                    child: Text(_outcomeFilterLabel(outcome))))
                .toList(growable: false),
            onChanged: (outcome) {
              if (outcome != null) setState(() => _outcome = outcome);
            },
          ),
        ),
        if (!isMobile)
          SizedBox(
            width: 165,
            child: DropdownButton<String>(
              isExpanded: true,
              value: selectedType,
              items: [
                DropdownMenuItem(
                    value: '', child: Text(translate('All connection types'))),
                ...connectionTypes.map(
                    (type) => DropdownMenuItem(value: type, child: Text(type))),
              ],
              onChanged: (type) =>
                  setState(() => _connectionType = type ?? ''),
            ),
          ),
        SizedBox(
          width: searchWidth,
          child: TextField(
            controller: _search,
            onChanged: (_) => setState(() {}),
            decoration: InputDecoration(
              isDense: true,
              labelText: translate('Search sessions'),
              prefixIcon: const Icon(Icons.search),
            ),
          ),
        ),
        OutlinedButton.icon(
          onPressed: _loading ? null : _load,
          icon: const Icon(Icons.refresh),
          label: Text(translate('Refresh')),
        ),
        ElevatedButton.icon(
          onPressed: widget.enabled && !_loading ? _exportCsv : null,
          icon: const Icon(Icons.file_download_outlined),
          label: Text(translate('Export CSV')),
        ),
      ],
    );
  }

  Widget _desktopTable(List<ClantoSessionRecord> records) {
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: DataTable(
        showCheckboxColumn: false,
        columns: [
          DataColumn(label: Text(translate('Date and time'))),
          DataColumn(label: Text(translate('Outcome'))),
          DataColumn(label: Text(translate('Device'))),
          DataColumn(label: Text(translate('Type'))),
          DataColumn(label: Text(translate('ID profile'))),
          DataColumn(label: Text(translate('Access'))),
          DataColumn(label: Text(translate('Duration'))),
          DataColumn(label: Text(translate('Files'))),
        ],
        rows: records
            .map((record) => DataRow(
                  onSelectChanged: (selected) {
                    if (selected == true) _showDetail(record);
                  },
                  cells: [
                    DataCell(Text(_date(record.startedAt))),
                    DataCell(_outcomeBadge(record)),
                    DataCell(Text(record.peerName.isEmpty
                        ? record.peerId
                        : record.peerId.isEmpty
                            ? record.peerName
                            : '${record.peerName}\n${record.peerId}')),
                    DataCell(Text(record.connectionType.isEmpty
                        ? '-'
                        : record.connectionType)),
                    DataCell(Text(_idProfileLabel(record.idProfile))),
                    DataCell(Text(record.accessMethod(translate))),
                    DataCell(Text(record.closed
                        ? _duration(record.durationSecs)
                        : '-')),
                    DataCell(Text('${record.files.length}')),
                  ],
                ))
            .toList(growable: false),
      ),
    );
  }

  Widget _mobileCards(List<ClantoSessionRecord> records) {
    return Column(
      children: records
          .map((record) => Card(
                child: InkWell(
                  onTap: () => _showDetail(record),
                  child: Padding(
                    padding: const EdgeInsets.all(12),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(children: [
                          Expanded(child: Text(record.peer)),
                          _outcomeBadge(record),
                        ]),
                        const SizedBox(height: 8),
                        Text(_date(record.startedAt)),
                        Text(
                            '${record.connectionType.isEmpty ? '-' : record.connectionType} · ${record.accessMethod(translate)}'),
                        Text(
                            '${translate('ID profile')}: ${_idProfileLabel(record.idProfile)}'),
                        Row(children: [
                          ..._permissionIcons(record),
                          const Icon(Icons.folder, size: 17),
                          const SizedBox(width: 4),
                          Text('${record.files.length}'),
                          const Spacer(),
                          Text(record.closed
                              ? _duration(record.durationSecs)
                              : '-'),
                        ]),
                      ],
                    ),
                  ),
                ),
              ))
          .toList(growable: false),
    );
  }

  @override
  Widget build(BuildContext context) {
    final records = _filteredRecords;
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      _filters(context),
      if (_loading)
        const Padding(
            padding: EdgeInsets.all(20), child: Center(child: CircularProgressIndicator()))
      else if (_error != null)
        Padding(
          padding: const EdgeInsets.all(12),
          child: Text(_error!, style: const TextStyle(color: Colors.red)),
        )
      else if (records.isEmpty)
        Padding(
          padding: const EdgeInsets.all(18),
          child: Text(translate('No sessions found')),
        )
      else if (isMobile)
        _mobileCards(records)
      else
        _desktopTable(records),
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
        title: Text(translate('Report sessions')),
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
