// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// «Collega al server»: iscrizione del PC a un tenant con QR a rotazione.
// La logica sta nel servizio (src/clanto/enrollment.rs): qui si legge lo
// stato e si mandano i comandi start, accept, reject, cancel.

import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:get/get.dart';
import 'package:qr_flutter/qr_flutter.dart';

const String _kStateKey = 'clanto-enrollment';
const String _kQrKey = 'clanto-enroll-qr';

class ClantoEnrollmentState {
  final bool enrolled;
  final bool apiConfigured;
  final String tenantName;
  final int enrolledAt;
  final String qrState;
  final String code;
  final String qr;
  final int codeExpiresAt;
  final String claimedBy;
  final String qrTenant;
  final String error;

  const ClantoEnrollmentState({
    this.enrolled = false,
    this.apiConfigured = false,
    this.tenantName = '',
    this.enrolledAt = 0,
    this.qrState = 'idle',
    this.code = '',
    this.qr = '',
    this.codeExpiresAt = 0,
    this.claimedBy = '',
    this.qrTenant = '',
    this.error = '',
  });

  factory ClantoEnrollmentState.read() {
    try {
      final v = jsonDecode(bind.mainGetCommonSync(key: _kStateKey));
      final q = (v['qr'] as Map?) ?? {};
      return ClantoEnrollmentState(
        enrolled: v['enrolled'] == true,
        apiConfigured: v['api_configured'] == true,
        tenantName: v['tenant_name'] ?? '',
        enrolledAt: v['enrolled_at'] ?? 0,
        qrState: (q['state'] ?? '').isEmpty ? 'idle' : q['state'],
        code: q['code'] ?? '',
        qr: q['qr'] ?? '',
        codeExpiresAt: q['code_expires_at'] ?? 0,
        claimedBy: q['claimed_by'] ?? '',
        qrTenant: q['tenant_name'] ?? '',
        error: q['error'] ?? '',
      );
    } catch (_) {
      return const ClantoEnrollmentState();
    }
  }
}

bool clantoIsAdmin() =>
    bind.mainGetCommonSync(key: 'clanto-is-admin') == 'true';

/// La scheda compare se il PC è già gestito o se l'interfaccia è avviata come
/// amministratore.
bool clantoEnrollmentVisible() {
  if (isWeb || bind.isOutgoingOnly()) return false;
  return clantoIsAdmin() || ClantoEnrollmentState.read().enrolled;
}

String _date(int unix) {
  if (unix <= 0) return '';
  final d = DateTime.fromMillisecondsSinceEpoch(unix * 1000);
  String two(int n) => n.toString().padLeft(2, '0');
  return '${two(d.day)}/${two(d.month)}/${d.year}';
}

class ClantoEnrollment extends StatefulWidget {
  const ClantoEnrollment({super.key});

  @override
  State<ClantoEnrollment> createState() => _ClantoEnrollmentState();
}

class _ClantoEnrollmentState extends State<ClantoEnrollment> {
  ClantoEnrollmentState _state = ClantoEnrollmentState.read();
  final bool _admin = clantoIsAdmin();
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    _timer = Timer.periodic(const Duration(seconds: 1), (_) {
      if (!mounted) return;
      setState(() => _state = ClantoEnrollmentState.read());
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    // Una sessione QR lasciata a metà non resta aperta.
    if (const ['starting', 'waiting', 'claimed'].contains(_state.qrState)) {
      bind.mainSetCommon(key: _kQrKey, value: 'cancel');
    }
    super.dispose();
  }

  void _send(String command) {
    bind.mainSetCommon(key: _kQrKey, value: command);
    setState(() => _state = ClantoEnrollmentState.read());
  }

  Widget _text(String key, {TextStyle? style}) =>
      Text(translate(key), style: style).marginOnly(bottom: 8);

  Widget _managed() {
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Row(children: [
        const Icon(Icons.verified_user, color: Colors.green),
        const SizedBox(width: 8),
        Expanded(
          child: Text(
            '${translate('clanto_enroll_managed_by')} ${_state.tenantName}',
            style: const TextStyle(fontWeight: FontWeight.bold),
          ),
        ),
      ]).marginOnly(bottom: 8),
      if (_state.enrolledAt > 0)
        Text('${translate('clanto_enroll_since')} ${_date(_state.enrolledAt)}')
            .marginOnly(bottom: 8),
      _text('clanto_enroll_managed_note'),
    ]);
  }

  Widget _qr() {
    final left = _state.codeExpiresAt -
        DateTime.now().millisecondsSinceEpoch ~/ 1000;
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      _text('clanto_enroll_scan'),
      if (_state.qr.isNotEmpty)
        Container(
          color: Colors.white,
          padding: const EdgeInsets.all(8),
          child: QrImageView(data: _state.qr, size: 200),
        ).marginOnly(bottom: 8),
      SelectableText(
        _state.code,
        style: const TextStyle(
            fontSize: 22, fontWeight: FontWeight.bold, letterSpacing: 2),
      ).marginOnly(bottom: 4),
      if (left > 0)
        Text('${translate('clanto_enroll_code_changes')} $left s')
            .marginOnly(bottom: 8),
      OutlinedButton(
          onPressed: () => _send('cancel'), child: Text(translate('Cancel'))),
    ]);
  }

  Widget _claimed() {
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Text(
        translate('clanto_enroll_confirm_title')
            .replaceAll('{tenant}', _state.qrTenant),
        style: const TextStyle(fontWeight: FontWeight.bold),
      ).marginOnly(bottom: 8),
      if (_state.claimedBy.isNotEmpty)
        Text('${translate('clanto_enroll_requested_by')} ${_state.claimedBy}')
            .marginOnly(bottom: 8),
      _text('clanto_enroll_confirm_note'),
      Row(children: [
        ElevatedButton(
            onPressed: () => _send('accept'),
            child: Text(translate('clanto_enroll_accept'))),
        const SizedBox(width: 12),
        OutlinedButton(
            onPressed: () => _send('reject'),
            child: Text(translate('clanto_enroll_reject'))),
      ]),
    ]);
  }

  Widget _start({String? message}) {
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      if (message != null)
        Text(message, style: const TextStyle(color: Colors.red))
            .marginOnly(bottom: 8),
      _text('clanto_enroll_intro'),
      ElevatedButton(
          onPressed: () => _send('start'),
          child: Text(translate('clanto_enroll_start'))),
    ]);
  }

  @override
  Widget build(BuildContext context) {
    if (_state.enrolled) return _managed();
    if (!_state.apiConfigured) return _text('clanto_enroll_no_server');
    if (!_admin) return _text('clanto_enroll_admin_required');
    switch (_state.qrState) {
      case 'starting':
        return const Align(
            alignment: Alignment.centerLeft,
            child: CircularProgressIndicator());
      case 'waiting':
        return _qr();
      case 'claimed':
        return _claimed();
      case 'done':
        return _managed();
      case 'expired':
        return _start(message: translate('clanto_enroll_expired'));
      case 'rejected':
        return _start(message: translate('clanto_enroll_rejected'));
      case 'error':
        return _start(
            message: '${translate('clanto_enroll_error')} ${_state.error}');
      default:
        return _start();
    }
  }
}
