import 'package:flutter_hbb/clanto/managed_data.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('Clanto codice di iscrizione', () {
    test('normalizza trattini, spazi e minuscole', () {
      expect(clantoNormalizeEnrollCode(' k7mx-q2pd '), 'K7MXQ2PD');
      expect(clantoEnrollCodeValid('K7MX-Q2PD'), isTrue);
      expect(clantoEnrollCodeValid('K7MXQ2PD'), isTrue);
      expect(clantoEnrollCodeValid('K7MX-Q2P'), isFalse);
      expect(clantoFormatEnrollCode('k7mxq2pd'), 'K7MX-Q2PD');
    });
  });

  group('Clanto dispositivi gestiti', () {
    test('legge un elemento di GET /me/devices', () {
      final d = ClantoManagedDevice.fromJson({
        'device_id': 'd1',
        'tenant_id': 't1',
        'tenant_name': 'Acme',
        'rustdesk_id': '123456789',
        'name': 'PC-01',
        'group': {'id': 'g1', 'name': 'Sede'},
        'online': true,
        'posture': 'warn',
        'effective': {
          'access_mode': 'account_2fa',
          'permissions': ['view', 'control'],
          'approval_required': true,
        },
      });
      expect(d.groupName, 'Sede');
      expect(d.online, isTrue);
      expect(d.posture, 'warn');
      expect(d.permissions, ['view', 'control']);
      expect(d.approvalRequired, isTrue);
    });

    test('campi mancanti: valori sicuri', () {
      final d = ClantoManagedDevice.fromJson({'posture': 'boh'});
      expect(d.posture, 'unknown');
      expect(d.online, isFalse);
      expect(d.approvalRequired, isFalse);
      expect(d.permissions, isEmpty);
    });
  });

  group('Clanto richieste di accesso', () {
    test('legge stato approvato con finestra', () {
      final r = ClantoAccessRequest.fromJson({
        'id': 'r1',
        'state': 'approved',
        'decided_by': {'id': 'u', 'display_name': 'Mario'},
        'approved': {'duration_s': 3600, 'expires_at': '2026-01-01T10:00:00Z'},
      });
      expect(r.isApproved, isTrue);
      expect(r.decidedBy, 'Mario');
      expect(r.approvedDurationS, 3600);
    });

    test('durate non oltre la richiesta', () {
      expect(clantoDurationChoices(3600), [900, 1800, 3600]);
      expect(clantoDurationChoices(1200), [900, 1200]);
      expect(clantoDurationChoices(0).last, 28800);
    });

    test('errori noti mappati su chiavi', () {
      expect(clantoErrorKey('self_approval', 403), 'clanto_err_self_approval');
      expect(clantoErrorKey('2fa_step_up_required', 403),
          'clanto_err_2fa_step_up_required');
      expect(clantoErrorKey(null, 401), 'clanto_err_login_required');
      expect(clantoErrorKey('boh', 500), isNull);
    });
  });
}
