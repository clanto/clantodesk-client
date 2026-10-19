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
          controllerPcName: 'PC-UFFICIO',
        ),
        r'ID 123 456 789 · m.rossi · ACME\mrossi su PC-UFFICIO',
      );
    });

    test('omits missing controller identity parts and separators', () {
      expect(
        buildClantoControllerIdentityLine(
          controllerId: '42',
          accountUser: ' ',
          osUser: '',
          controllerPcName: 'PC-UFFICIO',
        ),
        'ID 42 · PC-UFFICIO',
      );
      expect(
        buildClantoControllerIdentityLine(
          controllerId: '',
          accountUser: 'account',
          osUser: 'alice',
          controllerPcName: '',
        ),
        'account · alice',
      );
    });

    test('formats the host, Italian local date, and session code line', () {
      expect(
        buildClantoHostSessionLine(
          hostName: 'HOST-01',
          hostUser: 'mario',
          sessionCode: 'S-0123-4567',
          now: DateTime(2026, 10, 19, 7, 5),
        ),
        '→ HOST-01 (mario) · 19/10/2026 07:05 · S-0123-4567',
      );
    });

    test('omits missing host identity parts', () {
      expect(
        buildClantoHostSessionLine(
          hostName: '',
          hostUser: 'mario',
          sessionCode: '',
          now: DateTime(2026, 1, 2, 3, 4),
        ),
        '→ mario · 02/01/2026 03:04',
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
