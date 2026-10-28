// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Parte pura (senza dipendenze Flutter) di dispositivi gestiti, richieste di
// accesso e collegamento PC: modelli JSON, normalizzazione del codice QR,
// mappa errore API -> chiave di traduzione. Contratto: clantodesk-server
// docs/api-future.md (GET /me/devices, /access-requests, /enroll/qr/claim).

const String kClantoApiPrefix = '/api/clanto/v1';

/// Elemento di `GET /me/devices`.
class ClantoManagedDevice {
  final String deviceId;
  final String tenantId;
  final String tenantName;
  final String rustdeskId;
  final String name;
  final String groupName;
  final String os;
  final bool online;
  final String posture; // ok|warn|fail|unknown
  final String accessMode;
  final List<String> permissions;
  final bool approvalRequired;

  const ClantoManagedDevice({
    required this.deviceId,
    required this.tenantId,
    required this.tenantName,
    required this.rustdeskId,
    required this.name,
    required this.groupName,
    required this.os,
    required this.online,
    required this.posture,
    required this.accessMode,
    required this.permissions,
    required this.approvalRequired,
  });

  factory ClantoManagedDevice.fromJson(Map<String, dynamic> j) {
    final effective = j['effective'] is Map
        ? Map<String, dynamic>.from(j['effective'] as Map)
        : <String, dynamic>{};
    final group = j['group'] is Map ? j['group'] as Map : const {};
    final perms = effective['permissions'];
    return ClantoManagedDevice(
      deviceId: '${j['device_id'] ?? ''}',
      tenantId: '${j['tenant_id'] ?? ''}',
      tenantName: '${j['tenant_name'] ?? ''}',
      rustdeskId: '${j['rustdesk_id'] ?? ''}',
      name: '${j['name'] ?? ''}',
      groupName: '${group['name'] ?? ''}',
      os: '${j['os'] ?? ''}',
      online: j['online'] == true,
      posture: _oneOf('${j['posture'] ?? ''}', const ['ok', 'warn', 'fail']),
      accessMode: '${effective['access_mode'] ?? ''}',
      permissions:
          perms is List ? perms.map((e) => '$e').toList() : const <String>[],
      approvalRequired: effective['approval_required'] == true,
    );
  }
}

String _oneOf(String v, List<String> allowed) =>
    allowed.contains(v) ? v : 'unknown';

/// Richiesta JIT (`GET /access-requests/{id}` e voci dell'elenco approvatore).
class ClantoAccessRequest {
  final String id;
  final String state; // pending|approved|denied|expired|cancelled|used
  final String deviceId;
  final String deviceName;
  final String requester;
  final String purpose;
  final String ticket;
  final String requestedAt;
  final String decidedBy;
  final String decisionNote;
  final int requestedDurationS; // 0 = non indicata
  final int approvedDurationS;
  final String approvedExpiresAt;

  const ClantoAccessRequest({
    required this.id,
    required this.state,
    required this.deviceId,
    required this.deviceName,
    required this.requester,
    required this.purpose,
    required this.ticket,
    required this.requestedAt,
    required this.decidedBy,
    required this.decisionNote,
    required this.requestedDurationS,
    required this.approvedDurationS,
    required this.approvedExpiresAt,
  });

  bool get isPending => state == 'pending';
  bool get isApproved => state == 'approved';

  factory ClantoAccessRequest.fromJson(Map<String, dynamic> j) {
    final approved = j['approved'] is Map ? j['approved'] as Map : const {};
    final by = j['decided_by'] is Map ? j['decided_by'] as Map : const {};
    final requester = j['requester'] is Map
        ? (j['requester'] as Map)['display_name']
        : j['requester'];
    return ClantoAccessRequest(
      id: '${j['id'] ?? ''}',
      state: '${j['state'] ?? ''}',
      deviceId: '${j['device_id'] ?? ''}',
      deviceName: '${j['device_name'] ?? ''}',
      requester: '${requester ?? ''}',
      purpose: '${j['purpose'] ?? ''}',
      ticket: '${j['ticket'] ?? ''}',
      requestedAt: '${j['requested_at'] ?? ''}',
      decidedBy: '${by['display_name'] ?? ''}',
      decisionNote: '${j['decision_note'] ?? ''}',
      requestedDurationS: _toInt(j['duration_s'] ?? j['requested_duration_s']),
      approvedDurationS: _toInt(approved['duration_s']),
      approvedExpiresAt: '${approved['expires_at'] ?? ''}',
    );
  }
}

int _toInt(Object? v) => v is num ? v.toInt() : int.tryParse('$v') ?? 0;

/// Normalizza il codice di iscrizione: solo alfanumerici, maiuscolo.
String clantoNormalizeEnrollCode(String raw) =>
    raw.replaceAll(RegExp(r'[^A-Za-z0-9]'), '').toUpperCase();

/// Il codice mostrato sotto il QR ha 8 caratteri (K7MX-Q2PD).
bool clantoEnrollCodeValid(String raw) =>
    clantoNormalizeEnrollCode(raw).length == 8;

/// Formato di visualizzazione XXXX-XXXX (altrimenti il testo normalizzato).
String clantoFormatEnrollCode(String raw) {
  final c = clantoNormalizeEnrollCode(raw);
  return c.length > 4 ? '${c.substring(0, 4)}-${c.substring(4)}' : c;
}

/// Durate proponibili (secondi) fino a [maxS]; se [maxS] <= 0 nessun limite
/// noto e si usa l'elenco completo.
List<int> clantoDurationChoices(int maxS) {
  const all = [900, 1800, 3600, 7200, 14400, 28800];
  if (maxS <= 0) return all;
  final out = all.where((d) => d <= maxS).toList();
  if (!out.contains(maxS) && maxS <= all.last) out.add(maxS);
  return out.isEmpty ? [maxS] : out;
}

/// Etichetta breve di una durata: "15 min", "2 h" (unità da tradurre a parte).
({int value, bool hours}) clantoDurationParts(int seconds) =>
    seconds % 3600 == 0
        ? (value: seconds ~/ 3600, hours: true)
        : (value: (seconds / 60).ceil(), hours: false);

/// Codice errore API -> chiave di traduzione; null se sconosciuto.
String? clantoErrorKey(String? code, int status) {
  const known = {
    'self_approval',
    'not_approver',
    'already_decided',
    'expired',
    '2fa_step_up_required',
    'forbidden',
    'not_found',
    'request_pending',
    'invalid_duration',
    'invalid_code',
    'invalid_group',
    'group_required',
    'invalid_purpose',
    'invalid_ticket',
    'host_offline',
  };
  if (code != null && known.contains(code)) return 'clanto_err_$code';
  if (status == 401) return 'clanto_err_login_required';
  if (status == 429) return 'clanto_err_rate_limited';
  return null;
}
