// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:get/get.dart';

import '../common.dart';
import '../models/model.dart';

const String kOptClantoControllerWatermark = 'clanto-controller-watermark';
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
  required bool localEnabled,
  required bool hostRequired,
  required ClantoWatermarkSessionType sessionType,
}) {
  return (sessionType == ClantoWatermarkSessionType.remoteDesktop ||
          sessionType == ClantoWatermarkSessionType.viewCamera) &&
      (localEnabled || hostRequired);
}

String buildClantoWatermarkText({
  required String controllerId,
  required String accountUsername,
  required String computerName,
  required DateTime now,
}) {
  final identity = accountUsername.trim().isNotEmpty
      ? accountUsername.trim()
      : computerName.trim();
  final local = now.toLocal();
  String twoDigits(int value) => value.toString().padLeft(2, '0');
  final date = '${local.year}-${twoDigits(local.month)}-${twoDigits(local.day)}';
  final time = '${twoDigits(local.hour)}:${twoDigits(local.minute)}';
  return 'ClantoDesk ID: ${controllerId.trim()}  •  $identity  •  $date $time';
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
  final localEnabled =
      bind.mainGetOptionSync(key: kOptClantoControllerWatermark) == 'Y';
  final hostRequired = ffi.ffiModel.pi
          .platformAdditions[kPeerInfoClantoRequireControllerWatermark] ==
      true;
  if (!ffi.ffiModel.pi.isSet.value ||
      !shouldShowClantoWatermark(
        localEnabled: localEnabled,
        hostRequired: hostRequired,
        sessionType: _sessionType(ffi.connType),
      )) {
    return child;
  }
  return ClantoControllerWatermark(child: child);
}

class ClantoControllerWatermark extends StatefulWidget {
  final Widget child;

  const ClantoControllerWatermark({
    Key? key,
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
  String _computerName = '';
  DateTime _now = DateTime.now();

  @override
  void initState() {
    super.initState();
    final knownId = gFFI.serverModel.serverId.id.trim();
    if (knownId.isNotEmpty && !knownId.contains(translate('Generating ...'))) {
      _controllerId = knownId;
    }
    _computerName = _readComputerName();
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

  String _readComputerName() {
    try {
      final info = jsonDecode(bind.mainGetLoginDeviceInfo());
      if (info is Map<String, dynamic>) {
        return (info['name'] ?? '').toString();
      }
    } catch (error) {
      debugPrint('Failed to read controller computer name: $error');
    }
    return '';
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
            final text = buildClantoWatermarkText(
              controllerId: _controllerId,
              accountUsername: gFFI.userModel.userName.value,
              computerName: _computerName,
              now: _now,
            );
            return IgnorePointer(
              child: ClipRect(
                child: CustomPaint(painter: _ClantoWatermarkPainter(text)),
              ),
            );
          }),
        ),
      ],
    );
  }
}

class _ClantoWatermarkPainter extends CustomPainter {
  final String text;

  const _ClantoWatermarkPainter(this.text);

  @override
  void paint(Canvas canvas, Size size) {
    if (size.isEmpty || text.isEmpty) return;

    final outline = TextPainter(
      text: TextSpan(
        text: text,
        style: TextStyle(
          fontSize: 18,
          fontWeight: FontWeight.w600,
          letterSpacing: 0.4,
          foreground: Paint()
            ..style = PaintingStyle.stroke
            ..strokeWidth = 2.2
            ..color = Colors.black.withOpacity(0.28),
        ),
      ),
      textDirection: TextDirection.ltr,
      maxLines: 1,
    )..layout();
    final fill = TextPainter(
      text: TextSpan(
        text: text,
        style: TextStyle(
          color: Colors.white.withOpacity(0.24),
          fontSize: 18,
          fontWeight: FontWeight.w600,
          letterSpacing: 0.4,
        ),
      ),
      textDirection: TextDirection.ltr,
      maxLines: 1,
    )..layout();

    final extent = math.sqrt(size.width * size.width + size.height * size.height);
    final horizontalStep = math.max(fill.width + 90, 420.0).toDouble();
    const verticalStep = 105.0;
    canvas.save();
    canvas.translate(size.width / 2, size.height / 2);
    canvas.rotate(-math.pi / 6);
    for (double y = -extent; y <= extent; y += verticalStep) {
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
      oldDelegate.text != text;
}
