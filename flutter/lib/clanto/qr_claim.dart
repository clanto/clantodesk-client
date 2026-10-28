// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// "Collega un PC": l'amministratore digita il codice mostrato sotto il QR sul
// PC (nessuno scanner disponibile) e lo rivendica per un tenant
// (POST /enroll/qr/claim). La persona al PC deve poi accettare.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/clanto/api.dart';
import 'package:flutter_hbb/clanto/managed_data.dart';
import 'package:flutter_hbb/common.dart';

Future<void> showClantoQrClaim(BuildContext context) => clantoShowScreen(
      context,
      title: translate('clanto_link_pc'),
      child: const ClantoQrClaimView(),
    );

/// Forza maiuscolo e formato XXXX-XXXX durante la digitazione.
class _CodeFormatter extends TextInputFormatter {
  @override
  TextEditingValue formatEditUpdate(
      TextEditingValue oldValue, TextEditingValue newValue) {
    final c = clantoNormalizeEnrollCode(newValue.text);
    final text = clantoFormatEnrollCode(c.length > 8 ? c.substring(0, 8) : c);
    return TextEditingValue(
        text: text, selection: TextSelection.collapsed(offset: text.length));
  }
}

class ClantoQrClaimView extends StatefulWidget {
  const ClantoQrClaimView({super.key});

  @override
  State<ClantoQrClaimView> createState() => _ClantoQrClaimViewState();
}

class _ClantoQrClaimViewState extends State<ClantoQrClaimView> {
  final _code = TextEditingController();
  final _group = TextEditingController();
  Map<String, String> _tenants = {}; // uuid -> nome
  String? _tenant;
  bool _loading = true;
  bool _busy = false;
  bool _done = false;
  String? _error;
  bool _fatal = false; // errore nel caricamento dei tenant

  @override
  void initState() {
    super.initState();
    _loadTenants();
  }

  @override
  void dispose() {
    _code.dispose();
    _group.dispose();
    super.dispose();
  }

  Future<void> _loadTenants() async {
    setState(() {
      _loading = true;
      _error = null;
      _fatal = false;
    });
    final r = await clantoApi('GET', '/me');
    if (!mounted) return;
    setState(() {
      _loading = false;
      if (!r.ok) {
        _error = r.message;
        _fatal = true;
        return;
      }
      final names = r.json['tenant_names'] is Map
          ? Map<String, dynamic>.from(r.json['tenant_names'] as Map)
          : <String, dynamic>{};
      final ids = r.json['tenants'] is List
          ? (r.json['tenants'] as List).map((e) => '$e').toList()
          : <String>[];
      _tenants = {
        for (final id in ids)
          id: (names[id] is String && (names[id] as String).isNotEmpty)
              ? names[id] as String
              : id,
      };
      if (_tenants.isEmpty) {
        _error = translate('clanto_no_tenants');
        _fatal = true;
      } else if (_tenants.length == 1) {
        _tenant = _tenants.keys.first;
      }
    });
  }

  Future<void> _claim() async {
    if (!clantoEnrollCodeValid(_code.text)) {
      setState(() => _error = translate('clanto_code_invalid'));
      return;
    }
    if (_tenant == null) {
      setState(() => _error = translate('clanto_tenant_required'));
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    final group = _group.text.trim();
    final r = await clantoApi('POST', '/enroll/qr/claim', body: {
      'code': clantoNormalizeEnrollCode(_code.text),
      'tenant_id': _tenant,
      if (group.isNotEmpty) 'group_id': group,
    });
    if (!mounted) return;
    setState(() {
      _busy = false;
      if (r.ok) {
        _done = true;
      } else {
        _error = r.message;
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) return const Center(child: CircularProgressIndicator());
    if (_fatal) {
      return ClantoErrorBox(message: _error!, onRetry: _loadTenants);
    }
    if (_done) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            const Icon(Icons.check_circle_outline,
                color: Colors.green, size: 40),
            const SizedBox(height: 8),
            Text(translate('clanto_claim_ok'), textAlign: TextAlign.center),
            const SizedBox(height: 12),
            TextButton(
                onPressed: () => setState(() {
                      _done = false;
                      _code.clear();
                    }),
                child: Text(translate('clanto_link_another'))),
          ]),
        ),
      );
    }
    return SingleChildScrollView(
      padding: const EdgeInsets.all(16),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Text(translate('clanto_link_pc_help')),
        const SizedBox(height: 12),
        TextField(
          controller: _code,
          enabled: !_busy,
          autocorrect: false,
          enableSuggestions: false,
          textCapitalization: TextCapitalization.characters,
          inputFormatters: [_CodeFormatter()],
          style: const TextStyle(fontFamily: 'monospace', fontSize: 20),
          decoration: InputDecoration(
              labelText: translate('clanto_code'), hintText: 'K7MX-Q2PD'),
        ),
        if (_tenants.length > 1)
          DropdownButtonFormField<String>(
            value: _tenant,
            decoration: InputDecoration(labelText: translate('clanto_tenant')),
            items: _tenants.entries
                .map((e) =>
                    DropdownMenuItem(value: e.key, child: Text(e.value)))
                .toList(),
            onChanged: _busy ? null : (v) => setState(() => _tenant = v),
          ),
        TextField(
          controller: _group,
          enabled: !_busy,
          decoration: InputDecoration(labelText: translate('clanto_group_id')),
        ),
        if (_error != null)
          Padding(
            padding: const EdgeInsets.only(top: 12),
            child: Text(_error!,
                style: TextStyle(color: Theme.of(context).colorScheme.error)),
          ),
        const SizedBox(height: 16),
        FilledButton(
            onPressed: _busy ? null : _claim,
            child: Text(translate('clanto_claim'))),
      ]),
    );
  }
}
