import 'package:flutter_hbb/clanto/session_report_data.dart';
import 'package:flutter_test/flutter_test.dart';

ClantoSessionRecord record({
  required String peerId,
  required String peerName,
  required String ip,
  required String sessionCode,
  required String connectionType,
  bool rejected = false,
  bool closed = true,
}) {
  return ClantoSessionRecord(
    startedAt: null,
    endedAt: null,
    durationSecs: 0,
    sessionCode: sessionCode,
    peerId: peerId,
    peerName: peerName,
    ip: ip,
    connectionType: connectionType,
    idProfile: 'support',
    primaryAuth: '',
    twoFactor: '',
    postureResult: '',
    postureBypasses: const [],
    permissions: const <String>{},
    files: const [],
    closeReason: '',
    rejected: rejected,
    closed: closed,
  );
}

void main() {
  final records = [
    record(
      peerId: '100-200',
      peerName: 'Reception',
      ip: '192.0.2.10',
      sessionCode: 'S-AAAA-BBBB',
      connectionType: 'remote',
    ),
    record(
      peerId: '300-400',
      peerName: 'Warehouse',
      ip: '192.0.2.20',
      sessionCode: 'S-CCCC-DDDD',
      connectionType: 'file-transfer',
      rejected: true,
      closed: false,
    ),
    record(
      peerId: '500-600',
      peerName: 'Meeting room',
      ip: '192.0.2.30',
      sessionCode: 'S-EEEE-FFFF',
      connectionType: 'remote',
      closed: false,
    ),
  ];

  test('filters session records by outcome', () {
    expect(
      clantoFilterSessionRecords(records,
          outcome: ClantoSessionOutcomeFilter.successful),
      [records[0]],
    );
    expect(
      clantoFilterSessionRecords(records,
          outcome: ClantoSessionOutcomeFilter.rejected),
      [records[1]],
    );
    expect(
      clantoFilterSessionRecords(records,
          outcome: ClantoSessionOutcomeFilter.inProgress),
      [records[2]],
    );
  });

  test('filters session records by connection type', () {
    expect(
      clantoFilterSessionRecords(records, connectionType: 'REMOTE'),
      [records[0], records[2]],
    );
  });

  test('searches session ID, device name, IP, and session code', () {
    expect(clantoFilterSessionRecords(records, query: '100-200'), [records[0]]);
    expect(clantoFilterSessionRecords(records, query: 'warehouse'), [records[1]]);
    expect(clantoFilterSessionRecords(records, query: '192.0.2.30'), [records[2]]);
    expect(clantoFilterSessionRecords(records, query: 'cccc-dddd'), [records[1]]);
  });

  test('parses the authorized ID profile', () {
    final parsed = clantoParseSessionRecords('[{"id_profile":"custom"}]');

    expect(parsed.single.idProfile, 'custom');
  });

  test('exports only filtered session records as CSV', () {
    final filtered = clantoFilterSessionRecords(
      records,
      outcome: ClantoSessionOutcomeFilter.rejected,
    );
    final csv = clantoSessionRecordsCsv(
      filtered,
      formatDate: (_) => '-',
      translate: (value) => value,
    );

    expect(csv, contains('"300-400"'));
    expect(csv, contains('"S-CCCC-DDDD"'));
    expect(csv, contains('"support"'));
    expect(csv, isNot(contains('"100-200"')));
    expect(csv, isNot(contains('"500-600"')));
    expect(csv.split('\r\n').where((line) => line.isNotEmpty), hasLength(2));
  });
}
