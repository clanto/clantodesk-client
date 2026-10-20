// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'dart:convert';

const List<String> clantoIdPermissionKeys = [
  'keyboard',
  'clipboard',
  'file',
  'audio',
  'camera',
  'terminal',
  'tunnel',
  'restart',
  'recording',
  'block_input',
  'privacy_mode',
  'remote_printer',
];

enum ClantoIdProfile { view, support, full, custom }

class ClantoIdPermissionEntry {
  const ClantoIdPermissionEntry({
    required this.pattern,
    this.name = '',
    this.profile = ClantoIdProfile.full,
    this.permissions = const {},
  });

  final String pattern;
  final String name;
  final ClantoIdProfile profile;
  final Set<String> permissions;

  ClantoIdPermissionEntry copyWith({
    String? pattern,
    String? name,
    ClantoIdProfile? profile,
    Set<String>? permissions,
  }) {
    return ClantoIdPermissionEntry(
      pattern: pattern ?? this.pattern,
      name: name ?? this.name,
      profile: profile ?? this.profile,
      permissions: permissions ?? this.permissions,
    );
  }
}

class ClantoIdPermissionsDocument {
  const ClantoIdPermissionsDocument({
    required this.entries,
    required this.invalidJson,
  });

  final List<ClantoIdPermissionEntry> entries;
  final bool invalidJson;
}

List<String> clantoParseIdWhitelist(String value) {
  final seen = <String>{};
  return value
      .split(RegExp(r'[\s,;]+'))
      .map((entry) => entry.trim())
      .where((entry) => entry.isNotEmpty && seen.add(entry))
      .toList(growable: false);
}

String clantoSerializeIdWhitelist(Iterable<ClantoIdPermissionEntry> entries) {
  final seen = <String>{};
  return entries
      .map((entry) => entry.pattern.trim())
      .where((pattern) => pattern.isNotEmpty && seen.add(pattern))
      .join(',');
}

ClantoIdPermissionsDocument clantoParseIdPermissions(
  String raw,
  Iterable<String> whitelist,
) {
  final patterns = whitelist.toList(growable: false);
  if (raw.trim().isEmpty) {
    return ClantoIdPermissionsDocument(
      entries: patterns
          .map((pattern) => ClantoIdPermissionEntry(pattern: pattern))
          .toList(growable: false),
      invalidJson: false,
    );
  }

  try {
    final decoded = jsonDecode(raw);
    if (decoded is! Map) throw const FormatException();
    final profiles = decoded.map(
      (key, value) =>
          MapEntry(key.toString().toLowerCase(), _parseProfile(value)),
    );
    return ClantoIdPermissionsDocument(
      entries: patterns
          .map((pattern) => (profiles[pattern.toLowerCase()] ??
                  const ClantoIdPermissionEntry(pattern: ''))
              .copyWith(pattern: pattern))
          .toList(growable: false),
      invalidJson: false,
    );
  } on FormatException {
    return _invalidDocument(patterns);
  } on JsonUnsupportedObjectError {
    return _invalidDocument(patterns);
  }
}

ClantoIdPermissionEntry _parseProfile(dynamic value) {
  if (value is! Map) throw const FormatException();
  final name = value['name'];
  final rawProfile = value['profile'];
  if (name != null && name is! String) throw const FormatException();
  if (rawProfile is! String) throw const FormatException();
  ClantoIdProfile? profile;
  for (final candidate in ClantoIdProfile.values) {
    if (candidate.name == rawProfile) {
      profile = candidate;
      break;
    }
  }
  if (profile == null) throw const FormatException();

  final permissions = <String>{};
  final rawPermissions = value['perms'];
  if (rawPermissions != null && rawPermissions is! Map) {
    throw const FormatException();
  }
  if (rawPermissions is Map) {
    for (final permission in clantoIdPermissionKeys) {
      final configured = rawPermissions[permission];
      if (configured != null && configured is! bool) {
        throw const FormatException();
      }
      if (profile == ClantoIdProfile.custom && configured == true) {
        permissions.add(permission);
      }
    }
  }
  return ClantoIdPermissionEntry(
    pattern: '',
    name: name is String ? name : '',
    profile: profile,
    permissions: Set.unmodifiable(permissions),
  );
}

ClantoIdPermissionsDocument _invalidDocument(List<String> patterns) {
  return ClantoIdPermissionsDocument(
    entries: patterns
        .map((pattern) => ClantoIdPermissionEntry(
              pattern: pattern,
              profile: ClantoIdProfile.view,
            ))
        .toList(growable: false),
    invalidJson: true,
  );
}

String clantoSerializeIdPermissions(
  Iterable<ClantoIdPermissionEntry> entries,
) {
  final result = <String, dynamic>{};
  for (final entry in entries) {
    final pattern = entry.pattern.trim();
    if (pattern.isEmpty || result.containsKey(pattern)) continue;
    result[pattern] = <String, dynamic>{
      if (entry.name.trim().isNotEmpty) 'name': entry.name.trim(),
      'profile': entry.profile.name,
      if (entry.profile == ClantoIdProfile.custom)
        'perms': <String, bool>{
          for (final permission in clantoIdPermissionKeys)
            if (entry.permissions.contains(permission)) permission: true,
        },
    };
  }
  return jsonEncode(result);
}
