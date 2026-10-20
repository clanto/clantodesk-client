import 'package:flutter_hbb/clanto/id_permissions_data.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('uses full for whitelist entries without a configured profile', () {
    final document = clantoParseIdPermissions(
      '{"100*":{"name":"Reception","profile":"support"},'
      '"orphan":{"profile":"view"}}',
      ['100*', '200-300'],
    );

    expect(document.invalidJson, isFalse);
    expect(document.entries, hasLength(2));
    expect(document.entries[0].name, 'Reception');
    expect(document.entries[0].profile, ClantoIdProfile.support);
    expect(document.entries[1].profile, ClantoIdProfile.full);
    expect(document.entries.map((entry) => entry.pattern),
        isNot(contains('orphan')));
  });

  test('invalid JSON applies view to every whitelist entry', () {
    final document = clantoParseIdPermissions('{invalid', ['100', '200*']);

    expect(document.invalidJson, isTrue);
    expect(
      document.entries.map((entry) => entry.profile),
      everyElement(ClantoIdProfile.view),
    );
  });

  test('serializes only current rows and selected custom permissions', () {
    final entries = [
      const ClantoIdPermissionEntry(
        pattern: '100*',
        name: 'Reception',
        profile: ClantoIdProfile.custom,
        permissions: {'keyboard', 'clipboard'},
      ),
      const ClantoIdPermissionEntry(
        pattern: '200-300',
        profile: ClantoIdProfile.full,
      ),
    ];

    final encoded = clantoSerializeIdPermissions(entries);
    final roundTrip = clantoParseIdPermissions(
      encoded,
      clantoParseIdWhitelist(clantoSerializeIdWhitelist(entries)),
    );

    expect(roundTrip.invalidJson, isFalse);
    expect(roundTrip.entries.map((entry) => entry.pattern), ['100*', '200-300']);
    expect(roundTrip.entries[0].permissions, {'keyboard', 'clipboard'});
    expect(encoded, isNot(contains('orphan')));
  });
}
