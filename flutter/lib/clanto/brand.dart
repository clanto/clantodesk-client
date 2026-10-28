// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Brand dei partner, livello 1 (src/clanto/brand.rs): nome e link a vista.
// Senza brand valgono quelli di ClantoDesk.

import 'dart:convert';

import 'package:flutter_hbb/models/platform_model.dart';

class ClantoBrand {
  const ClantoBrand._(
      this.branded, this.name, this.supportUrl, this.websiteUrl);

  final bool branded;
  final String name;
  final String supportUrl;
  final String websiteUrl;

  static ClantoBrand? _current;

  static ClantoBrand get current => _current ??= _load();

  static ClantoBrand _load() {
    Map<String, dynamic> values = {};
    try {
      values = jsonDecode(bind.mainGetCommonSync(key: 'clanto-brand'))
          as Map<String, dynamic>;
    } catch (_) {}
    String value(String key, String fallback) {
      final v = values[key];
      return v is String && v.isNotEmpty ? v : fallback;
    }

    return ClantoBrand._(
      values['branded'] == true,
      value('name', 'ClantoDesk'),
      value('support-url', 'https://supporto.clanto.it/clantodesk'),
      value('website-url', 'https://clanto.it'),
    );
  }
}
