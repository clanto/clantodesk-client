// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Accesso just-in-time: richiesta del tecnico (POST /access-requests, stato in
// long-poll) e vista dell'approvatore (elenco, approva/nega). Alla
// connessione il livello nativo richiede da solo il grant di sessione.

import 'package:flutter/material.dart';
import 'package:flutter_hbb/clanto/api.dart';
import 'package:flutter_hbb/clanto/managed_data.dart';
import 'package:flutter_hbb/common.dart';

const int kClantoPurposeMax = 500;
const int kClantoTicketMax = 128;
const int kClantoNoteMax = 500;
const int kClantoWaitSeconds = 25;

/// Dialogo di richiesta per [device]; dopo l'invio mostra lo stato.
Future<void> showClantoAccessRequestDialog(
    BuildContext context, ClantoManagedDevice device) {
  return showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (context) => _AccessRequestDialog(device: device),
  );
}

class _AccessRequestDialog extends StatefulWidget {
  const _AccessRequestDialog({required this.device});

  final ClantoManagedDevice device;

  @override
  State<_AccessRequestDialog> createState() => _AccessRequestDialogState();
}

class _AccessRequestDialogState extends State<_AccessRequestDialog> {
  final _purpose = TextEditingController();
  final _ticket = TextEditingController();
  int _duration = 3600;
  bool _busy = false;
  String? _error;
  String? _requestId;
  int _approvers = 0;

  @override
  void dispose() {
    _purpose.dispose();
    _ticket.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    final purpose = _purpose.text.trim();
    if (purpose.isEmpty) {
      setState(() => _error = translate('clanto_purpose_required'));
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    final r = await clantoApi('POST', '/access-requests', body: {
      'device_id': widget.device.deviceId,
      'purpose': purpose,
      if (_ticket.text.trim().isNotEmpty) 'ticket': _ticket.text.trim(),
      'duration_s': _duration,
    });
    if (!mounted) return;
    if (r.ok && r.json['id'] is String) {
      setState(() {
        _busy = false;
        _requestId = r.json['id'] as String;
        _approvers = (r.json['approvers'] as num?)?.toInt() ?? 0;
      });
    } else {
      setState(() {
        _busy = false;
        _error = r.message;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final title = Text(
        '${translate('clanto_request_title')}: ${widget.device.name.isEmpty ? widget.device.rustdeskId : widget.device.name}');
    if (_requestId != null) {
      return AlertDialog(
        title: title,
        content: SizedBox(
          width: 440,
          child: ClantoRequestStatus(
            requestId: _requestId!,
            rustdeskId: widget.device.rustdeskId,
            approvers: _approvers,
            onConnect: () {
              Navigator.of(context).pop();
              connect(context, widget.device.rustdeskId);
            },
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: Text(translate('Close'))),
        ],
      );
    }
    return AlertDialog(
      title: title,
      content: SizedBox(
        width: 440,
        child: SingleChildScrollView(
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            TextField(
              controller: _purpose,
              enabled: !_busy,
              maxLength: kClantoPurposeMax,
              maxLines: 3,
              minLines: 2,
              decoration:
                  InputDecoration(labelText: translate('clanto_purpose')),
            ),
            TextField(
              controller: _ticket,
              enabled: !_busy,
              maxLength: kClantoTicketMax,
              decoration:
                  InputDecoration(labelText: translate('clanto_ticket')),
            ),
            const SizedBox(height: 8),
            DropdownButtonFormField<int>(
              value: _duration,
              decoration:
                  InputDecoration(labelText: translate('clanto_duration')),
              items: clantoDurationChoices(0)
                  .map((d) => DropdownMenuItem(
                      value: d, child: Text(clantoDurationLabel(d))))
                  .toList(),
              onChanged:
                  _busy ? null : (v) => setState(() => _duration = v ?? 3600),
            ),
            if (_error != null)
              Padding(
                padding: const EdgeInsets.only(top: 12),
                child: Text(_error!,
                    style:
                        TextStyle(color: Theme.of(context).colorScheme.error)),
              ),
          ]),
        ),
      ),
      actions: [
        TextButton(
            onPressed: _busy ? null : () => Navigator.of(context).pop(),
            child: Text(translate('Cancel'))),
        FilledButton(
            onPressed: _busy ? null : _submit,
            child: Text(translate('clanto_send_request'))),
      ],
    );
  }
}

String clantoRequestStateLabel(String state) {
  const known = [
    'pending',
    'approved',
    'denied',
    'expired',
    'cancelled',
    'used'
  ];
  return known.contains(state) ? translate('clanto_state_$state') : state;
}

/// Stato di una richiesta con long-poll fino al cambio di stato.
class ClantoRequestStatus extends StatefulWidget {
  const ClantoRequestStatus({
    super.key,
    required this.requestId,
    required this.rustdeskId,
    required this.approvers,
    required this.onConnect,
  });

  final String requestId;
  final String rustdeskId;
  final int approvers;
  final VoidCallback onConnect;

  @override
  State<ClantoRequestStatus> createState() => _ClantoRequestStatusState();
}

class _ClantoRequestStatusState extends State<ClantoRequestStatus> {
  ClantoAccessRequest? _req;
  String? _error;
  bool _polling = false;
  bool _disposed = false;

  @override
  void initState() {
    super.initState();
    _poll();
  }

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }

  Future<void> _poll() async {
    if (_polling) return;
    _polling = true;
    _error = null;
    var failures = 0;
    while (!_disposed) {
      final wait = _req == null ? 0 : kClantoWaitSeconds;
      final r = await clantoApi('GET', '/access-requests/${widget.requestId}',
          query: {'wait': '$wait'});
      if (_disposed) break;
      if (r.ok) {
        failures = 0;
        final req = ClantoAccessRequest.fromJson(r.json);
        setState(() => _req = req);
        if (!req.isPending) break;
      } else if (r.unauthorized || (!r.networkError && r.status < 500)) {
        setState(() => _error = r.message);
        break;
      } else if (++failures >= 5) {
        setState(() => _error = r.message);
        break;
      } else {
        await Future.delayed(const Duration(seconds: 3));
      }
    }
    _polling = false;
  }

  @override
  Widget build(BuildContext context) {
    final req = _req;
    if (_error != null) {
      return ClantoErrorBox(
          message: _error!,
          onRetry: () {
            setState(() => _error = null);
            _poll();
          });
    }
    if (req == null) {
      return const Padding(
          padding: EdgeInsets.all(24),
          child: Center(child: CircularProgressIndicator()));
    }
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(children: [
          Icon(
              req.isApproved
                  ? Icons.check_circle_outline
                  : req.isPending
                      ? Icons.hourglass_top
                      : Icons.block,
              color: req.isApproved ? Colors.green : null),
          const SizedBox(width: 8),
          Text(clantoRequestStateLabel(req.state),
              style: const TextStyle(fontWeight: FontWeight.w600)),
        ]),
        const SizedBox(height: 8),
        if (req.isPending && widget.approvers > 0)
          Text('${translate('clanto_approvers_notified')}: ${widget.approvers}'),
        if (req.decidedBy.isNotEmpty)
          Text('${translate('clanto_decided_by')}: ${req.decidedBy}'),
        if (req.decisionNote.isNotEmpty)
          Text('${translate('clanto_note')}: ${req.decisionNote}'),
        if (req.isApproved) ...[
          if (req.approvedExpiresAt.isNotEmpty)
            Text(
                '${translate('clanto_window_until')}: ${req.approvedExpiresAt}'),
          if (req.approvedDurationS > 0)
            Text(
                '${translate('clanto_duration')}: ${clantoDurationLabel(req.approvedDurationS)}'),
          const SizedBox(height: 12),
          FilledButton(
              onPressed: widget.onConnect,
              child: Text(translate('clanto_connect'))),
        ],
      ],
    );
  }
}

/// Schermata dell'approvatore.
Future<void> showClantoApprovals(BuildContext context) => clantoShowScreen(
      context,
      title: translate('clanto_approvals'),
      child: const ClantoApprovalsView(),
    );

class ClantoApprovalsView extends StatefulWidget {
  const ClantoApprovalsView({super.key});

  @override
  State<ClantoApprovalsView> createState() => _ClantoApprovalsViewState();
}

class _ClantoApprovalsViewState extends State<ClantoApprovalsView> {
  List<ClantoAccessRequest> _items = [];
  bool _loading = true;
  String? _error;
  String? _info;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    final r = await clantoApi('GET', '/access-requests',
        query: {'role': 'approver', 'state': 'pending'});
    if (!mounted) return;
    setState(() {
      _loading = false;
      if (r.ok) {
        final data = r.json['data'];
        _items = data is List
            ? data
                .whereType<Map>()
                .map((e) =>
                    ClantoAccessRequest.fromJson(Map<String, dynamic>.from(e)))
                .toList()
            : [];
      } else {
        _error = r.message;
      }
    });
  }

  Future<void> _decide(ClantoAccessRequest req, bool approve) async {
    final result = await showDialog<_Decision>(
      context: context,
      builder: (context) => _DecisionDialog(req: req, approve: approve),
    );
    if (result == null || !mounted) return;
    final r = await clantoApi(
        'POST', '/access-requests/${req.id}/${approve ? 'approve' : 'deny'}',
        body: {
          if (approve) 'duration_s': result.durationS,
          if (result.note.isNotEmpty) 'note': result.note,
        });
    if (!mounted) return;
    if (r.ok) {
      setState(() => _info = translate(
          approve ? 'clanto_decision_approved' : 'clanto_decision_denied'));
      _load();
    } else {
      setState(() => _info = r.message);
      // Richiesta già decisa o scaduta: l'elenco va aggiornato.
      if (r.errorCode == 'already_decided' || r.errorCode == 'expired') {
        _load();
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) return const Center(child: CircularProgressIndicator());
    if (_error != null) return ClantoErrorBox(message: _error!, onRetry: _load);
    return Column(children: [
      if (_info != null)
        Padding(
            padding: const EdgeInsets.all(8),
            child: Text(_info!, textAlign: TextAlign.center)),
      Expanded(
        child: _items.isEmpty
            ? Center(child: Text(translate('clanto_approvals_empty')))
            : RefreshIndicator(
                onRefresh: _load,
                child: ListView.separated(
                  itemCount: _items.length,
                  separatorBuilder: (_, __) => const Divider(height: 1),
                  itemBuilder: (context, i) => _tile(_items[i]),
                ),
              ),
      ),
    ]);
  }

  Widget _tile(ClantoAccessRequest r) {
    final device = r.deviceName.isNotEmpty ? r.deviceName : r.deviceId;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 12),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(device, style: const TextStyle(fontWeight: FontWeight.w600)),
        if (r.requester.isNotEmpty)
          Text('${translate('clanto_requester')}: ${r.requester}'),
        Text(r.purpose),
        if (r.ticket.isNotEmpty)
          Text('${translate('clanto_ticket')}: ${r.ticket}'),
        if (r.requestedDurationS > 0)
          Text(
              '${translate('clanto_duration')}: ${clantoDurationLabel(r.requestedDurationS)}'),
        if (r.requestedAt.isNotEmpty)
          Text(r.requestedAt,
              style: TextStyle(color: Theme.of(context).hintColor)),
        const SizedBox(height: 4),
        Row(children: [
          FilledButton(
              onPressed: () => _decide(r, true),
              child: Text(translate('clanto_approve'))),
          const SizedBox(width: 8),
          OutlinedButton(
              onPressed: () => _decide(r, false),
              child: Text(translate('clanto_deny'))),
        ]),
      ]),
    );
  }
}

class _Decision {
  final int durationS;
  final String note;

  const _Decision(this.durationS, this.note);
}

class _DecisionDialog extends StatefulWidget {
  const _DecisionDialog({required this.req, required this.approve});

  final ClantoAccessRequest req;
  final bool approve;

  @override
  State<_DecisionDialog> createState() => _DecisionDialogState();
}

class _DecisionDialogState extends State<_DecisionDialog> {
  final _note = TextEditingController();
  late final List<int> _choices =
      clantoDurationChoices(widget.req.requestedDurationS);
  late int _duration = _choices.contains(3600) ? 3600 : _choices.last;

  @override
  void dispose() {
    _note.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(translate(widget.approve ? 'clanto_approve' : 'clanto_deny')),
      content: SizedBox(
        width: 400,
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          if (widget.approve)
            DropdownButtonFormField<int>(
              value: _duration,
              decoration: InputDecoration(
                  labelText: translate('clanto_approve_duration')),
              items: _choices
                  .map((d) => DropdownMenuItem(
                      value: d, child: Text(clantoDurationLabel(d))))
                  .toList(),
              onChanged: (v) => setState(() => _duration = v ?? _duration),
            ),
          TextField(
            controller: _note,
            maxLength: kClantoNoteMax,
            maxLines: 3,
            minLines: 1,
            decoration:
                InputDecoration(labelText: translate('clanto_decision_note')),
          ),
        ]),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(translate('Cancel'))),
        FilledButton(
            onPressed: () => Navigator.of(context)
                .pop(_Decision(_duration, _note.text.trim())),
            child: Text(
                translate(widget.approve ? 'clanto_approve' : 'clanto_deny'))),
      ],
    );
  }
}
