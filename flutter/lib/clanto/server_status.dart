// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Barra di stato: a quale server si collega il PC e se è un server Clanto
// (quello fissato al build, src/clanto/config.rs).

import 'dart:convert';

import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/models/state_model.dart';

class ClantoServer {
  final String host;
  final bool official;
  const ClantoServer(this.host, this.official);
}

ClantoServer clantoServerStatus() {
  try {
    final json = jsonDecode(bind.mainClantoServerStatus());
    return ClantoServer(json['host'] ?? '', json['official'] == true);
  } catch (_) {
    return const ClantoServer('', false);
  }
}

String clantoConnStatusText(SvcStatus status, ClantoServer server) {
  if (!server.official) {
    final host = server.host.isEmpty ? '' : ': ${server.host}';
    return switch (status) {
      SvcStatus.ready => '${translate('Ready, server not managed by Clanto')}$host',
      SvcStatus.connecting => '${translate('connecting_status')}$host',
      _ => '${translate('not_ready_status')}$host',
    };
  }
  return switch (status) {
    SvcStatus.ready => translate('Ready to connect through the Clanto server'),
    SvcStatus.connecting => translate('Connecting to the Clanto server…'),
    _ => translate('Clanto server not reachable'),
  };
}
