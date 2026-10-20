import 'package:flutter_hbb/clanto/watermark.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('Clanto controller watermark', () {
    test('formats the controller identity line', () {
      expect(
        buildClantoControllerIdentityLine(
          controllerId: '123 456 789',
          accountUser: 'm.rossi',
          osUser: r'ACME\mrossi',
          sessionCode: 'S-0123-4567',
        ),
        'm.rossi · ID 123 456 789 · S-0123-4567',
      );
    });

    test('removes Windows and email domains from the OS user fallback', () {
      expect(
        buildClantoControllerIdentityLine(
          controllerId: '42',
          accountUser: ' ',
          osUser: r'BURBACCHIOTTO\Antonio',
          sessionCode: 'S-AAAA-BBBB',
        ),
        'Antonio · ID 42 · S-AAAA-BBBB',
      );
      expect(
        buildClantoControllerIdentityLine(
          controllerId: '',
          accountUser: '',
          osUser: 'utente@dominio',
          sessionCode: '',
        ),
        'utente',
      );
    });

    test('omits missing controller identity parts and separators', () {
      expect(
        buildClantoControllerIdentityLine(
          controllerId: '42',
          accountUser: ' ',
          osUser: '',
          sessionCode: ' ',
        ),
        'ID 42',
      );
    });

    test('formats the host and Italian local date line', () {
      expect(
        buildClantoHostSessionLine(
          hostName: 'HOST-01',
          now: DateTime(2026, 10, 19, 7, 5),
        ),
        '→ HOST-01 · 19/10/2026 07:05',
      );
    });

    test('omits an empty host without leaving an arrow or separator', () {
      expect(
        buildClantoHostSessionLine(
          hostName: '',
          now: DateTime(2026, 1, 2, 3, 4),
        ),
        '02/01/2026 03:04',
      );
    });

    test('only the host requirement enables desktop and camera watermarks', () {
      for (final type in [
        ClantoWatermarkSessionType.remoteDesktop,
        ClantoWatermarkSessionType.viewCamera,
      ]) {
        expect(
          shouldShowClantoWatermark(hostRequired: false, sessionType: type),
          isFalse,
        );
        expect(
          shouldShowClantoWatermark(hostRequired: true, sessionType: type),
          isTrue,
        );
      }
    });

    test('non-screen session types never show the watermark', () {
      for (final type in ClantoWatermarkSessionType.values.where(
        (type) =>
            type != ClantoWatermarkSessionType.remoteDesktop &&
            type != ClantoWatermarkSessionType.viewCamera,
      )) {
        expect(
          shouldShowClantoWatermark(hostRequired: true, sessionType: type),
          isFalse,
          reason: type.name,
        );
      }
    });
  });
}
