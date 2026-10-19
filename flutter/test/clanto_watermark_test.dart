import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/clanto/watermark.dart';

void main() {
  group('Clanto controller watermark', () {
    test('uses account username and formats the current local minute', () {
      final text = buildClantoWatermarkText(
        controllerId: '123 456 789',
        accountUsername: 'm.rossi',
        computerName: 'PC-UFFICIO',
        now: DateTime(2026, 10, 19, 7, 5),
      );

      expect(text,
          'ClantoDesk ID: 123 456 789  •  m.rossi  •  2026-10-19 07:05');
    });

    test('falls back to the computer name when no account is logged in', () {
      final text = buildClantoWatermarkText(
        controllerId: '42',
        accountUsername: '   ',
        computerName: 'PC-UFFICIO',
        now: DateTime(2026, 1, 2, 3, 4),
      );

      expect(text, contains('42  •  PC-UFFICIO  •  2026-01-02 03:04'));
    });

    test('local option or host requirement enables remote desktop watermark', () {
      expect(
        shouldShowClantoWatermark(
          localEnabled: false,
          hostRequired: false,
          sessionType: ClantoWatermarkSessionType.remoteDesktop,
        ),
        isFalse,
      );
      expect(
        shouldShowClantoWatermark(
          localEnabled: true,
          hostRequired: false,
          sessionType: ClantoWatermarkSessionType.remoteDesktop,
        ),
        isTrue,
      );
      expect(
        shouldShowClantoWatermark(
          localEnabled: false,
          hostRequired: true,
          sessionType: ClantoWatermarkSessionType.remoteDesktop,
        ),
        isTrue,
      );
    });

    test('local option or host requirement enables camera watermark', () {
      expect(
        shouldShowClantoWatermark(
          localEnabled: true,
          hostRequired: false,
          sessionType: ClantoWatermarkSessionType.viewCamera,
        ),
        isTrue,
      );
      expect(
        shouldShowClantoWatermark(
          localEnabled: false,
          hostRequired: true,
          sessionType: ClantoWatermarkSessionType.viewCamera,
        ),
        isTrue,
      );
    });

    test('non-screen session types never show the watermark', () {
      for (final type in ClantoWatermarkSessionType.values.where(
          (type) =>
              type != ClantoWatermarkSessionType.remoteDesktop &&
              type != ClantoWatermarkSessionType.viewCamera)) {
        expect(
          shouldShowClantoWatermark(
            localEnabled: true,
            hostRequired: true,
            sessionType: type,
          ),
          isFalse,
          reason: type.name,
        );
      }
    });
  });
}
