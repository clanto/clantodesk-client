// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'dart:convert';

enum ClantoSessionOutcome { successful, rejected, inProgress }

enum ClantoSessionOutcomeFilter { all, successful, rejected, inProgress }

class ClantoSessionRecord {
  const ClantoSessionRecord({
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

  factory ClantoSessionRecord.fromJson(Map<String, dynamic> json) {
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
    return ClantoSessionRecord(
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
        return 'Approval';
      case 2:
        return 'Temporary password';
      case 3:
        return 'Permanent password';
      case 4:
        return 'Switch sides';
      default:
        return '';
    }
  }

  static String _twoFactor(dynamic value) {
    switch (_integer(value)) {
      case 1:
        return 'TOTP';
      case 2:
        return 'Trusted device';
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

  String accessMethod(String Function(String) translate) {
    final methods = [
      if (primaryAuth.isNotEmpty) translate(primaryAuth),
      if (twoFactor.isNotEmpty) translate(twoFactor),
    ];
    return methods.isEmpty ? '-' : methods.join(' + ');
  }

  ClantoSessionOutcome get outcome {
    if (rejected) return ClantoSessionOutcome.rejected;
    if (!closed) return ClantoSessionOutcome.inProgress;
    return ClantoSessionOutcome.successful;
  }

  String statusLabel(String Function(String) translate) =>
      closed ? translate('Ended') : translate('Not closed');
}

List<ClantoSessionRecord> clantoParseSessionRecords(String raw) {
  final decoded = jsonDecode(raw);
  final List<dynamic> values = decoded is List
      ? decoded
      : decoded is Map && decoded['records'] is List
          ? decoded['records'] as List<dynamic>
          : const [];
  return values
      .whereType<Map>()
      .map((value) => ClantoSessionRecord.fromJson(
          value.map((key, item) => MapEntry(key.toString(), item))))
      .toList(growable: false);
}

List<ClantoSessionRecord> clantoFilterSessionRecords(
  Iterable<ClantoSessionRecord> records, {
  ClantoSessionOutcomeFilter outcome = ClantoSessionOutcomeFilter.all,
  String connectionType = '',
  String query = '',
}) {
  final normalizedType = connectionType.trim().toLowerCase();
  final normalizedQuery = query.trim().toLowerCase();
  return records.where((record) {
    final matchesOutcome = switch (outcome) {
      ClantoSessionOutcomeFilter.all => true,
      ClantoSessionOutcomeFilter.successful =>
        record.outcome == ClantoSessionOutcome.successful,
      ClantoSessionOutcomeFilter.rejected =>
        record.outcome == ClantoSessionOutcome.rejected,
      ClantoSessionOutcomeFilter.inProgress =>
        record.outcome == ClantoSessionOutcome.inProgress,
    };
    if (!matchesOutcome) return false;
    if (normalizedType.isNotEmpty &&
        record.connectionType.toLowerCase() != normalizedType) {
      return false;
    }
    if (normalizedQuery.isEmpty) return true;
    return [record.peerId, record.peerName, record.ip, record.sessionCode]
        .any((value) => value.toLowerCase().contains(normalizedQuery));
  }).toList(growable: false);
}

String clantoSessionRecordsCsv(
  Iterable<ClantoSessionRecord> records, {
  required String Function(DateTime?) formatDate,
  required String Function(String) translate,
}) {
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
    ...records.map((record) => [
          formatDate(record.startedAt),
          formatDate(record.endedAt),
          record.statusLabel(translate),
          record.closed ? '${record.durationSecs}' : '',
          record.sessionCode,
          record.peerId,
          record.peerName,
          record.ip,
          record.connectionType,
          record.accessMethod(translate),
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
