// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Forwarding Clanto (src/clanto/forwarding): impostazioni della policy e
// gestione dei forward di una sessione. Il port forward di upstream resta a parte.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/common/widgets/toolbar.dart';
import 'package:flutter_hbb/models/model.dart';
import 'package:flutter_hbb/models/platform_model.dart';

const String kOptClantoForwardEnabled = 'clanto-forward-enabled';
const String kOptClantoForwardCatalog = 'clanto-forward-catalog';
const String kOptClantoForwardCustomHost = 'clanto-forward-custom-host';
const String kOptClantoForwardCustomHostTargets =
    'clanto-forward-custom-host-targets';
const String kOptClantoForwardReverse = 'clanto-forward-reverse';
const String kOptClantoForwardCustomController =
    'clanto-forward-custom-controller';
const String kOptClantoForwardCustomControllerTargets =
    'clanto-forward-custom-controller-targets';
const String kOptClantoForwardCamera = 'clanto-forward-camera';
const String kOptClantoForwardCameraDevice = 'clanto-forward-camera-device';

bool _isY(String key) => bind.mainGetOptionSync(key: key) == 'Y';

/// Policy del forwarding su questo PC (host e controller).
class ClantoForwardingSettings extends StatefulWidget {
  final bool enabled;
  const ClantoForwardingSettings({Key? key, this.enabled = true})
      : super(key: key);

  @override
  State<ClantoForwardingSettings> createState() =>
      _ClantoForwardingSettingsState();
}

class _ClantoForwardingSettingsState extends State<ClantoForwardingSettings> {
  final _hostTargets = TextEditingController(
      text: bind.mainGetOptionSync(key: kOptClantoForwardCustomHostTargets));
  final _controllerTargets = TextEditingController(
      text: bind.mainGetOptionSync(
          key: kOptClantoForwardCustomControllerTargets));
  final _cameraDevice = TextEditingController(
      text: bind.mainGetOptionSync(key: kOptClantoForwardCameraDevice));
  String? _macCameraStatus;

  /// macOS: attiva la Camera Extension (flutter/macos/Runner/AppDelegate.swift).
  Future<void> _activateMacCamera() async {
    String status;
    try {
      final res = await const MethodChannel('it.clanto.clantodesk/camera_extension')
          .invokeMethod<String>('activate');
      status = translate(res == 'approval'
          ? 'Approve ClantoDesk Camera in System Settings, General, Login Items & Extensions'
          : 'Virtual camera active');
    } on PlatformException catch (e) {
      status = translate(e.message ?? e.code);
    } on MissingPluginException {
      status = translate('The virtual camera is not included in this build');
    }
    if (mounted) setState(() => _macCameraStatus = status);
  }

  @override
  void dispose() {
    _hostTargets.dispose();
    _controllerTargets.dispose();
    _cameraDevice.dispose();
    super.dispose();
  }

  bool _editable(String key) => widget.enabled && !isOptionFixed(key);

  Widget _check(String label, String key, {bool onByDefault = false}) {
    final value = onByDefault
        ? bind.mainGetOptionSync(key: key) != 'N'
        : _isY(key);
    return CheckboxListTile(
      dense: true,
      contentPadding: EdgeInsets.zero,
      controlAffinity: ListTileControlAffinity.leading,
      title: Text(translate(label)),
      value: value,
      onChanged: _editable(key)
          ? (v) async {
              await bind.mainSetOption(key: key, value: v == true ? 'Y' : 'N');
              if (mounted) setState(() {});
            }
          : null,
    );
  }

  Widget _targets(String label, String key, TextEditingController c,
      {String hint = 'localhost:3389, 192.168.10.0/24:443'}) {
    return Padding(
      padding: const EdgeInsets.only(left: 32, bottom: 8),
      child: TextField(
        controller: c,
        enabled: _editable(key),
        decoration: InputDecoration(
          labelText: translate(label),
          hintText: hint,
        ),
        onSubmitted: (v) => bind.mainSetOption(key: key, value: v.trim()),
        onTapOutside: (_) => bind.mainSetOption(key: key, value: c.text.trim()),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final on = _isY(kOptClantoForwardEnabled);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _check('Enable forwarding', kOptClantoForwardEnabled),
        if (on) ...[
          _check('Use the Clanto service catalog', kOptClantoForwardCatalog,
              onByDefault: true),
          _check('Allow custom forwards to this PC\'s network',
              kOptClantoForwardCustomHost),
          if (_isY(kOptClantoForwardCustomHost))
            _targets('Allowed destinations on this PC\'s side',
                kOptClantoForwardCustomHostTargets, _hostTargets),
          _check('Allow forwards towards the controller',
              kOptClantoForwardReverse),
          _check('Allow custom forwards from this PC\'s network as controller',
              kOptClantoForwardCustomController),
          if (_isY(kOptClantoForwardCustomController))
            _targets('Allowed destinations on the controller\'s side',
                kOptClantoForwardCustomControllerTargets, _controllerTargets),
          _check('Allow the controller\'s camera on this PC',
              kOptClantoForwardCamera),
          if (Platform.isLinux && _isY(kOptClantoForwardCamera))
            _targets('Virtual camera device (empty = automatic)',
                kOptClantoForwardCameraDevice, _cameraDevice,
                hint: '/dev/video10'),
          if (Platform.isMacOS && _isY(kOptClantoForwardCamera))
            Padding(
              padding: const EdgeInsets.only(left: 32, bottom: 8),
              child: Row(children: [
                TextButton(
                    onPressed: widget.enabled ? _activateMacCamera : null,
                    child: Text(translate('Activate the virtual camera'))),
                if (_macCameraStatus != null)
                  Flexible(child: Text(_macCameraStatus!)),
              ]),
            ),
        ],
      ],
    );
  }
}

/// Voce "Forwarding" nel menu della sessione.
TTextMenu clantoForwardingMenu(BuildContext context, FFI ffi) {
  return TTextMenu(
    child: Text(translate('Forwarding')),
    onPressed: () => showDialog<void>(
      context: context,
      builder: (_) => _ForwardingDialog(sessionId: ffi.sessionId),
    ),
  );
}

class _Service {
  final String id, name, direction, target;
  _Service(this.id, this.name, this.direction, this.target);
}

class _ForwardingDialog extends StatefulWidget {
  final SessionID sessionId;
  const _ForwardingDialog({required this.sessionId});

  @override
  State<_ForwardingDialog> createState() => _ForwardingDialogState();
}

class _ForwardingDialogState extends State<_ForwardingDialog> {
  List<Map<String, dynamic>> _forwards = [];
  List<_Service> _catalog = [];
  Timer? _timer;
  String _direction = 'to_host';
  String _serviceId = '';
  final _target = TextEditingController();
  final _localPort = TextEditingController(text: '0');
  String? _error;
  List<Map<String, dynamic>> _cameras = [];
  int? _cameraIndex;
  Map<String, dynamic>? _camera;
  String? _cameraError;

  @override
  void initState() {
    super.initState();
    bind.mainClantoCameras().then((json) {
      final res = jsonDecode(json);
      if (!mounted) return;
      setState(() {
        if (res['error'] != null) {
          _cameraError = translate(res['error']);
        } else {
          _cameras = (res['ok'] as List).cast<Map<String, dynamic>>();
          _cameraIndex = _cameras.isEmpty ? null : _cameras.first['index'];
        }
      });
    });
    _refresh();
    _timer = Timer.periodic(const Duration(seconds: 1), (_) => _refresh());
    bind.mainClantoForwardCatalog().then((json) {
      final list = (jsonDecode(json) as List)
          .map((s) => _Service(s['id'] ?? '', s['name'] ?? '',
              s['direction'] ?? '', s['target'] ?? ''))
          .toList();
      if (mounted) setState(() => _catalog = list);
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    _target.dispose();
    _localPort.dispose();
    super.dispose();
  }

  void _refresh() {
    final json = bind.sessionClantoForwardList(sessionId: widget.sessionId);
    final list = (jsonDecode(json) as List).cast<Map<String, dynamic>>();
    final camera = jsonDecode(
        bind.sessionClantoCameraStatus(sessionId: widget.sessionId));
    if (mounted) {
      setState(() {
        _forwards = list;
        _camera = camera;
      });
    }
  }

  void _add() {
    String host;
    int port;
    if (_serviceId.isNotEmpty) {
      final s = _catalog.firstWhere((s) => s.id == _serviceId);
      final i = s.target.lastIndexOf(':');
      host = s.target.substring(0, i);
      port = int.tryParse(s.target.substring(i + 1)) ?? 0;
    } else {
      final t = _target.text.trim();
      final i = t.lastIndexOf(':');
      if (i <= 0) {
        setState(() => _error = translate('Destination must be host:port'));
        return;
      }
      host = t.substring(0, i);
      port = int.tryParse(t.substring(i + 1)) ?? 0;
    }
    final res = jsonDecode(bind.sessionClantoForwardAdd(
      sessionId: widget.sessionId,
      direction: _direction,
      localPort: int.tryParse(_localPort.text.trim()) ?? 0,
      targetHost: host,
      targetPort: port,
      service: _serviceId,
    ));
    setState(() => _error = res['error'] == null ? null : translate(res['error']));
    _refresh();
  }

  bool get _cameraRunning =>
      _camera != null &&
      ['starting', 'pending', 'active'].contains(_camera!['state']);

  Future<void> _cameraStart() async {
    final index = _cameraIndex;
    if (index == null) return;
    if (isAndroid &&
        !await AndroidPermissionManager.request('android.permission.CAMERA')) {
      if (mounted) {
        setState(() => _cameraError = translate('Camera access denied'));
      }
      return;
    }
    final res = jsonDecode(await bind.sessionClantoCameraStart(
        sessionId: widget.sessionId, index: index));
    String? error = res['error'];
    // Telefoni: la cattura è nativa (ClantoCamera.kt / ClantoCamera.swift);
    // 1 = camera anteriore.
    if (error == null && isMobile) {
      try {
        error = await _nativeCamera?.$1.invokeMethod<String>(
            _nativeCamera!.$2, index == 1);
      } on PlatformException catch (e) {
        error = e.message ?? e.code;
      }
      if (error != null) {
        bind.sessionClantoCameraStop(sessionId: widget.sessionId);
      }
    }
    if (!mounted) return;
    setState(() => _cameraError = error == null ? null : translate(error));
    _refresh();
  }

  /// Canale e metodo di avvio della cattura nativa; null sul desktop.
  (MethodChannel, String)? get _nativeCamera {
    if (isAndroid) return (const MethodChannel('mChannel'), 'clanto_camera_start');
    if (isIOS) {
      return (const MethodChannel('it.clanto.clantodesk/camera'), 'start');
    }
    return null;
  }

  void _cameraStop() {
    bind.sessionClantoCameraStop(sessionId: widget.sessionId);
    if (isAndroid) {
      const MethodChannel('mChannel').invokeMethod('clanto_camera_stop');
    } else if (isIOS) {
      const MethodChannel('it.clanto.clantodesk/camera').invokeMethod('stop');
    }
    _refresh();
  }

  Widget _cameraSection() {
    final c = _camera;
    String? status;
    if (c != null) {
      status = translate(c['state'] ?? '');
      if (c['state'] == 'active') {
        status += ' ${c['width']}x${c['height']} → ${c['sink']}';
      }
      final message = c['message'] ?? '';
      if (message.isNotEmpty) status += ' ${translate(message)}';
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(translate('My camera on the remote PC'),
            style: const TextStyle(fontWeight: FontWeight.bold)),
        if (_cameras.isNotEmpty)
          Row(children: [
            Expanded(
              child: DropdownButton<int>(
                value: _cameraIndex,
                isExpanded: true,
                items: _cameras
                    .map((cam) => DropdownMenuItem<int>(
                        value: cam['index'], child: Text(cam['name'] ?? '')))
                    .toList(),
                onChanged: _cameraRunning
                    ? null
                    : (v) => setState(() => _cameraIndex = v),
              ),
            ),
            const SizedBox(width: 8),
            _cameraRunning
                ? TextButton(
                    onPressed: _cameraStop,
                    child: Text(translate('Stop')))
                : TextButton(
                    onPressed: _cameraStart, child: Text(translate('Start'))),
          ])
        else if (_cameraError == null)
          Text(translate('No camera found')),
        if (status != null) Text(status),
        if (_cameraError != null)
          Text(_cameraError!, style: const TextStyle(color: Colors.red)),
        const Divider(),
      ],
    );
  }

  String _describe(Map<String, dynamic> f) {
    final target = '${f['target_host']}:${f['target_port']}';
    final port = f['listen_port'];
    return f['direction'] == 'to_host'
        ? 'localhost:$port → $target (${translate('remote PC')})'
        : '${translate('remote PC')} localhost:$port → $target';
  }

  @override
  Widget build(BuildContext context) {
    final services = _catalog.where((s) => s.direction == _direction).toList();
    return AlertDialog(
      title: Text(translate('Forwarding')),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 560, maxHeight: 600),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              _cameraSection(),
              if (_forwards.isEmpty) Text(translate('No forwards')),
              ..._forwards.map((f) => ListTile(
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    title: SelectableText(_describe(f)),
                    subtitle: Text(
                        '${translate(f['state'] ?? '')} ${f['message'] ?? ''}'
                            .trim()),
                    trailing: IconButton(
                      icon: const Icon(Icons.delete_outline),
                      tooltip: translate('Delete'),
                      onPressed: () {
                        bind.sessionClantoForwardRemove(
                            sessionId: widget.sessionId, id: f['id']);
                        _refresh();
                      },
                    ),
                  )),
              const Divider(),
              DropdownButton<String>(
                value: _direction,
                isExpanded: true,
                items: [
                  DropdownMenuItem(
                      value: 'to_host',
                      child: Text(translate('From this PC to the remote PC\'s network'))),
                  DropdownMenuItem(
                      value: 'to_controller',
                      child: Text(translate('From the remote PC to this PC\'s network'))),
                ],
                onChanged: (v) => setState(() {
                  _direction = v ?? 'to_host';
                  _serviceId = '';
                }),
              ),
              if (services.isNotEmpty)
                DropdownButton<String>(
                  value: _serviceId,
                  isExpanded: true,
                  items: [
                    DropdownMenuItem(
                        value: '', child: Text(translate('Custom destination'))),
                    ...services.map((s) => DropdownMenuItem(
                        value: s.id, child: Text('${s.name} (${s.target})'))),
                  ],
                  onChanged: (v) => setState(() => _serviceId = v ?? ''),
                ),
              if (_serviceId.isEmpty)
                TextField(
                  controller: _target,
                  decoration: InputDecoration(
                      labelText: translate('Destination (host:port)'),
                      hintText: 'localhost:3389'),
                ),
              TextField(
                controller: _localPort,
                keyboardType: TextInputType.number,
                inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                decoration: InputDecoration(
                  labelText: translate(_direction == 'to_host'
                      ? 'Local port on this PC (0 = automatic)'
                      : 'Port on the remote PC (0 = automatic)'),
                ),
              ),
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: Text(_error!, style: const TextStyle(color: Colors.red)),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: _add, child: Text(translate('Add'))),
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(translate('Close')),
        ),
      ],
    );
  }
}
