import 'package:flutter_hbb/clanto/posture_login_error.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('recognizes and removes the invisible outdated marker', () {
    final parsed = clantoParsePostureLoginError(
      '${clantoOutdatedLoginErrorMarker}Update ClantoDesk.',
    );

    expect(parsed.requiresUpdate, isTrue);
    expect(parsed.message, 'Update ClantoDesk.');
  });

  test('leaves ordinary login errors unchanged', () {
    const message = 'The official ClantoDesk client is required.';
    final parsed = clantoParsePostureLoginError(message);

    expect(parsed.requiresUpdate, isFalse);
    expect(parsed.message, message);
  });
}
