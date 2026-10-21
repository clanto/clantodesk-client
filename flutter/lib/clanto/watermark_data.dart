// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

const String kOptClantoRequireControllerWatermark =
    'clanto-require-controller-watermark';
const String kPeerInfoClantoRequireControllerWatermark =
    'clanto-require-controller-watermark';

enum ClantoWatermarkSessionType {
  remoteDesktop,
  viewCamera,
  fileTransfer,
  terminal,
  portForward,
  other,
}

bool shouldShowClantoWatermark({
  required bool hostRequired,
  required ClantoWatermarkSessionType sessionType,
}) {
  return (sessionType == ClantoWatermarkSessionType.remoteDesktop ||
          sessionType == ClantoWatermarkSessionType.viewCamera) &&
      hostRequired;
}

String _oneLine(String value) =>
    value.trim().replaceAll('\r', ' ').replaceAll('\n', ' ');

String buildClantoControllerIdentityLine({
  required String controllerId,
  required String accountUser,
  required String osUser,
  required String sessionCode,
  String idLabel = 'ID',
}) {
  final account = _oneLine(accountUser);
  final os = _oneLine(osUser);
  final id = _oneLine(controllerId);
  final code = _oneLine(sessionCode);
  final withoutWindowsDomain = os.contains('\\') ? os.split('\\').last : os;
  final withoutDomain = withoutWindowsDomain.contains('@')
      ? withoutWindowsDomain.split('@').first
      : withoutWindowsDomain;
  final name = account.isNotEmpty ? account : withoutDomain.trim();
  final parts = <String>[
    if (name.isNotEmpty) name,
    if (id.isNotEmpty) '$idLabel $id',
    if (code.isNotEmpty) code,
  ];
  return parts.join(' · ');
}

String buildClantoHostSessionLine({
  required String hostName,
  required DateTime now,
}) {
  final host = _oneLine(hostName);
  final local = now.toLocal();
  String twoDigits(int value) => value.toString().padLeft(2, '0');
  final dateTime = '${twoDigits(local.day)}/${twoDigits(local.month)}/${local.year} '
      '${twoDigits(local.hour)}:${twoDigits(local.minute)}';
  return [
    if (host.isNotEmpty) '→ $host',
    dateTime,
  ].join(' · ');
}
