// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:get/get.dart';

import '../common.dart';
import '../common/formatter/id_formatter.dart';
import '../models/model.dart';
import '../models/platform_model.dart';

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
    if (id.isNotEmpty) '${translate('ID')} $id',
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

ClantoWatermarkSessionType _sessionType(ConnType connType) {
  switch (connType) {
    case ConnType.defaultConn:
      return ClantoWatermarkSessionType.remoteDesktop;
    case ConnType.fileTransfer:
      return ClantoWatermarkSessionType.fileTransfer;
    case ConnType.terminal:
      return ClantoWatermarkSessionType.terminal;
    case ConnType.portForward:
    case ConnType.rdp:
      return ClantoWatermarkSessionType.portForward;
    case ConnType.viewCamera:
      return ClantoWatermarkSessionType.viewCamera;
  }
}

Widget clantoControllerWatermark({
  required FFI ffi,
  required Widget child,
}) {
  final hostRequired = ffi.ffiModel.pi
          .platformAdditions[kPeerInfoClantoRequireControllerWatermark] ==
      true;
  if (!ffi.ffiModel.pi.isSet.value ||
      !shouldShowClantoWatermark(
        hostRequired: hostRequired,
        sessionType: _sessionType(ffi.connType),
      )) {
    return child;
  }
  return ClantoControllerWatermark(ffi: ffi, child: child);
}

class ClantoControllerWatermark extends StatefulWidget {
  final FFI ffi;
  final Widget child;

  const ClantoControllerWatermark({
    Key? key,
    required this.ffi,
    required this.child,
  }) : super(key: key);

  @override
  State<ClantoControllerWatermark> createState() =>
      _ClantoControllerWatermarkState();
}

class _ClantoControllerWatermarkState
    extends State<ClantoControllerWatermark> {
  Timer? _minuteTimer;
  String _controllerId = '';
  String _osUser = '';
  DateTime _now = DateTime.now();

  @override
  void initState() {
    super.initState();
    final knownId = trimID(gFFI.serverModel.serverId.text).trim();
    if (knownId.isNotEmpty && !knownId.contains(translate('Generating ...'))) {
      _controllerId = knownId;
    }
    _osUser = _readOsUser();
    _loadControllerId();
    final untilNextMinute = const Duration(minutes: 1) -
        Duration(seconds: _now.second, milliseconds: _now.millisecond);
    _minuteTimer = Timer(untilNextMinute, () {
      if (!mounted) return;
      setState(() => _now = DateTime.now());
      _minuteTimer = Timer.periodic(const Duration(minutes: 1), (_) {
        if (mounted) setState(() => _now = DateTime.now());
      });
    });
  }

  Future<void> _loadControllerId() async {
    try {
      final id = await bind.mainGetMyId();
      if (mounted) setState(() => _controllerId = id);
    } catch (error) {
      debugPrint('Failed to read controller ID: $error');
    }
  }

  String _readOsUser() {
    return bind.mainGetClantoControllerOsUser().trim();
  }

  @override
  void dispose() {
    _minuteTimer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Stack(
      fit: StackFit.passthrough,
      children: [
        widget.child,
        Positioned.fill(
          child: Obx(() {
            final lines = [
              buildClantoControllerIdentityLine(
                controllerId: _controllerId,
                accountUser: gFFI.userModel.userName.value,
                osUser: _osUser,
                sessionCode: widget.ffi.ffiModel.pi.clantoSessionCode,
              ),
              buildClantoHostSessionLine(
                hostName: widget.ffi.ffiModel.pi.hostname,
                now: _now,
              ),
            ].where((line) => line.isNotEmpty).toList(growable: false);
            return IgnorePointer(
              child: ClipRect(
                child: CustomPaint(painter: _ClantoWatermarkPainter(lines)),
              ),
            );
          }),
        ),
      ],
    );
  }
}

class _ClantoWatermarkPainter extends CustomPainter {
  final List<String> lines;

  const _ClantoWatermarkPainter(this.lines);

  @override
  void paint(Canvas canvas, Size size) {
    if (size.isEmpty || lines.isEmpty) return;

    TextPainter painter(String text, bool outline) => TextPainter(
      text: TextSpan(
        text: text,
        style: TextStyle(
          color: outline ? null : Colors.white.withOpacity(0.12),
          fontSize: 16,
          fontWeight: FontWeight.w600,
          letterSpacing: 0.4,
          foreground: outline
              ? (Paint()
                ..style = PaintingStyle.stroke
                ..strokeWidth = 1.5
                ..color = Colors.black.withOpacity(0.12))
              : null,
        ),
      ),
      textDirection: TextDirection.ltr,
      maxLines: 1,
    )..layout();

    final extent = math.sqrt(size.width * size.width + size.height * size.height);
    const verticalStep = 170.0;
    canvas.save();
    canvas.translate(size.width / 2, size.height / 2);
    canvas.rotate(-math.pi / 6);
    for (double y = -extent; y <= extent; y += verticalStep) {
      final lineIndex = (y / verticalStep).round().abs() % lines.length;
      final outline = painter(lines[lineIndex], true);
      final fill = painter(lines[lineIndex], false);
      final horizontalStep = math.max(fill.width + 120, 520.0).toDouble();
      final stagger = ((y / verticalStep).round().isEven)
          ? -extent
          : -extent + horizontalStep / 2;
      for (double x = stagger; x <= extent; x += horizontalStep) {
        final offset = Offset(x, y);
        outline.paint(canvas, offset);
        fill.paint(canvas, offset);
      }
    }
    canvas.restore();
  }

  @override
  bool shouldRepaint(covariant _ClantoWatermarkPainter oldDelegate) =>
      !listEquals(oldDelegate.lines, lines);
}
