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
