// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

const clantoOutdatedLoginErrorMarker = '\u2063\u2064\u2063';

class ClantoPostureLoginError {
  const ClantoPostureLoginError({
    required this.message,
    required this.requiresUpdate,
  });

  final String message;
  final bool requiresUpdate;
}

ClantoPostureLoginError clantoParsePostureLoginError(String message) {
  final requiresUpdate = message.startsWith(clantoOutdatedLoginErrorMarker);
  return ClantoPostureLoginError(
    message: requiresUpdate
        ? message.substring(clantoOutdatedLoginErrorMarker.length)
        : message,
    requiresUpdate: requiresUpdate,
  );
}
