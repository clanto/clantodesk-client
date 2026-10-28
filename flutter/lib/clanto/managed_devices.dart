// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// "Dispositivi gestiti": PC raggiungibili dall'utente (GET /me/devices) con
// stato, postura e permessi effettivi. Tocco = connessione all'ID RustDesk,
// oppure richiesta di accesso se il criterio impone l'approvazione.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_hbb/clanto/access_requests.dart';
import 'package:flutter_hbb/clanto/api.dart';
import 'package:flutter_hbb/clanto/managed_data.dart';
import 'package:flutter_hbb/common.dart';

const int kClantoDevicesPageSize = 50;

Future<void> showClantoManagedDevices(BuildContext context) =>
    clantoShowScreen(
      context,
      title: translate('clanto_managed_devices'),
      child: const ClantoManagedDevicesView(),
    );

class ClantoManagedDevicesView extends StatefulWidget {
  const ClantoManagedDevicesView({super.key});

  @override
  State<ClantoManagedDevicesView> createState() =>
      _ClantoManagedDevicesViewState();
}

class _ClantoManagedDevicesViewState extends State<ClantoManagedDevicesView> {
  final _search = TextEditingController();
  final List<ClantoManagedDevice> _items = [];
  Timer? _debounce;
  bool _onlineOnly = false;
  bool _loading = false;
  String? _error;
  int _total = 0;
  int _generation = 0; // scarta le risposte di ricerche superate

  @override
  void initState() {
    super.initState();
    _reload();
  }

  @override
  void dispose() {
    _debounce?.cancel();
    _search.dispose();
    super.dispose();
  }

  void _onSearchChanged(String _) {
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 400), _reload);
  }

  Future<void> _reload() async {
    _items.clear();
    _total = 0;
    await _load();
  }

  Future<void> _load() async {
    final q = _search.text.trim();
    final gen = ++_generation;
    setState(() {
      _loading = true;
      _error = null;
    });
    final r = await clantoApi('GET', '/me/devices', query: {
      if (q.length >= 2) 'q': q,
      if (_onlineOnly) 'online': 'true',
      'limit': '$kClantoDevicesPageSize',
      'offset': '${_items.length}',
    });
    if (!mounted || gen != _generation) return;
    setState(() {
      _loading = false;
      if (r.ok) {
        _total = (r.json['total'] as num?)?.toInt() ?? 0;
        final data = r.json['data'];
        if (data is List) {
          _items.addAll(data.whereType<Map>().map(
              (e) => ClantoManagedDevice.fromJson(Map<String, dynamic>.from(e))));
        }
      } else {
        _error = r.message;
      }
    });
  }

  void _open(ClantoManagedDevice d) {
    if (d.approvalRequired) {
      showClantoAccessRequestDialog(context, d);
      return;
    }
    if (isDesktop) Navigator.of(context).pop();
    connect(context, d.rustdeskId);
  }

  @override
  Widget build(BuildContext context) {
    return Column(children: [
      Padding(
        padding: const EdgeInsets.fromLTRB(12, 8, 12, 0),
        child: Row(children: [
          Expanded(
            child: TextField(
              controller: _search,
              onChanged: _onSearchChanged,
              decoration: InputDecoration(
                prefixIcon: const Icon(Icons.search),
                hintText: translate('clanto_search_hint'),
              ),
            ),
          ),
          const SizedBox(width: 12),
          FilterChip(
            label: Text(translate('clanto_only_online')),
            selected: _onlineOnly,
            onSelected: (v) {
              setState(() => _onlineOnly = v);
              _reload();
            },
          ),
        ]),
      ),
      Expanded(child: _body()),
    ]);
  }

  Widget _body() {
    if (_error != null && _items.isEmpty) {
      return ClantoErrorBox(message: _error!, onRetry: _reload);
    }
    if (_items.isEmpty) {
      return Center(
          child: _loading
              ? const CircularProgressIndicator()
              : Text(translate('clanto_no_devices')));
    }
    final more = _items.length < _total;
    return ListView.separated(
      itemCount: _items.length + (more || _error != null ? 1 : 0),
      separatorBuilder: (_, __) => const Divider(height: 1),
      itemBuilder: (context, i) {
        if (i < _items.length) return _tile(_items[i]);
        if (_error != null) {
          return ClantoErrorBox(message: _error!, onRetry: _load);
        }
        return Padding(
          padding: const EdgeInsets.all(8),
          child: Center(
            child: _loading
                ? const CircularProgressIndicator()
                : TextButton(
                    onPressed: _load,
                    child: Text(translate('clanto_load_more'))),
          ),
        );
      },
    );
  }

  Widget _tile(ClantoManagedDevice d) {
    final scheme = Theme.of(context).colorScheme;
    final where = [
      if (d.tenantName.isNotEmpty) d.tenantName,
      if (d.groupName.isNotEmpty) d.groupName,
    ].join(' / ');
    final perms = d.permissions.map(_permLabel).join(', ');
    return ListTile(
      leading: Icon(Icons.circle,
          size: 12, color: d.online ? Colors.green : scheme.outline),
      title: Text(d.name.isEmpty ? d.rustdeskId : d.name),
      subtitle: Text([
        d.rustdeskId,
        if (where.isNotEmpty) where,
        if (d.accessMode.isNotEmpty)
          '${translate('clanto_access_mode')}: ${d.accessMode}',
        if (perms.isNotEmpty) '${translate('clanto_permissions')}: $perms',
        translate(d.online ? 'clanto_online' : 'clanto_offline'),
      ].join('\n')),
      isThreeLine: true,
      trailing: Wrap(
        spacing: 8,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          _postureBadge(d.posture),
          if (d.approvalRequired)
            OutlinedButton(
                onPressed: () => _open(d),
                child: Text(translate('clanto_request_access')))
          else
            const Icon(Icons.chevron_right),
        ],
      ),
      onTap: () => _open(d),
    );
  }

  String _permLabel(String p) {
    const known = ['view', 'control', 'file', 'clipboard', 'audio'];
    return known.contains(p) ? translate('clanto_perm_$p') : p;
  }

  Widget _postureBadge(String posture) {
    final color = switch (posture) {
      'ok' => Colors.green,
      'warn' => Colors.orange,
      'fail' => Colors.red,
      _ => Colors.grey,
    };
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
      decoration: BoxDecoration(
        border: Border.all(color: color),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Text(translate('clanto_posture_$posture'),
          style: TextStyle(color: color, fontSize: 12)),
    );
  }
}
