// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Forwarding Clanto (src/clanto/forwarding): impostazioni e menu della sessione.
// Due funzioni separate: Forwarding rete (porte) e Forwarding webcam. Il port
// forward di upstream resta a parte.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/common/widgets/toolbar.dart';
import 'package:flutter_hbb/models/model.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:settings_ui/settings_ui.dart';

const String kOptClantoForwardEnabled = 'clanto-forward-enabled';
const String kOptClantoForwardServicesHost = 'clanto-forward-services-host';
const String kOptClantoForwardServicesController =
    'clanto-forward-services-controller';
const String kOptClantoForwardCustomHost = 'clanto-forward-custom-host';
const String kOptClantoForwardCustomHostTargets =
    'clanto-forward-custom-host-targets';
const String kOptClantoForwardReverse = 'clanto-forward-reverse';
const String kOptClantoForwardCustomController =
    'clanto-forward-custom-controller';
const String kOptClantoForwardCustomControllerTargets =
    'clanto-forward-custom-controller-targets';
const String kOptClantoForwardWebcam = 'clanto-forward-webcam';
const String kOptClantoForwardWebcamSend = 'clanto-forward-webcam-send';
const String kOptClantoForwardWebcamReceive = 'clanto-forward-webcam-receive';
const String kOptClantoForwardCameraDevice = 'clanto-forward-camera-device';

bool _isY(String key) => bind.mainGetOptionSync(key: key) == 'Y';

/// Servizi spuntati: `id=destinazione` separati da `;` (policy.rs).
Map<String, String> _enabledServices(String key) {
  final map = <String, String>{};
  for (final entry in bind.mainGetOptionSync(key: key).split(';')) {
    final i = entry.indexOf('=');
    if (i > 0) map[entry.substring(0, i).trim()] = entry.substring(i + 1).trim();
  }
  return map;
}

/// Elenco servizi Clanto (catalogo pubblicato, senza firma).
Future<List<_Service>> _loadCatalog() async {
  try {
    final list = jsonDecode(await bind.mainClantoForwardCatalog()) as List;
    return list
        .map((s) => _Service(s['id'] ?? '', s['name'] ?? '',
            s['direction'] ?? '', s['target'] ?? '', s['description'] ?? ''))
        .toList();
  } catch (_) {
    return [];
  }
}

/// La webcam del PC controllato arriva su una camera virtuale di questo PC:
/// solo sul desktop.
bool get _canReceiveWebcam => isDesktop;

/// Impostazioni del forwarding su questo PC: rete e webcam, separate.
class ClantoForwardingSettings extends StatefulWidget {
  final bool enabled;

  /// Telefoni: solo le opzioni da controller (non ricevono forward né webcam).
  final bool controllerOnly;
  const ClantoForwardingSettings(
      {Key? key, this.enabled = true, this.controllerOnly = false})
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
  List<_Service>? _catalog;

  @override
  void initState() {
    super.initState();
    _loadCatalog().then((list) {
      if (mounted) setState(() => _catalog = list);
    });
  }

  Future<void> _toggleService(
      String key, String id, String target, bool enable) async {
    final map = _enabledServices(key);
    if (enable) {
      map[id] = target;
    } else {
      map.remove(id);
    }
    await bind.mainSetOption(
        key: key,
        value: map.entries.map((e) => '${e.key}=${e.value}').join(';'));
    if (mounted) setState(() {});
  }

  /// Servizi del catalogo per una direzione: nessuno è attivo finché non lo
  /// si spunta. Le voci attive non più nell'elenco (o con un'altra
  /// destinazione) restano visibili per poterle togliere.
  Widget _services(String label, String key, String direction) {
    final enabled = _enabledServices(key);
    final editable = _editable(key);
    final list = (_catalog ?? []).where((s) => s.direction == direction);
    final orphans = enabled.entries.where(
        (e) => !list.any((s) => s.id == e.key && s.target == e.value));
    Widget tile(String title, String subtitle, bool value,
            ValueChanged<bool>? onChanged) =>
        CheckboxListTile(
          dense: true,
          contentPadding: const EdgeInsets.only(left: 8),
          controlAffinity: ListTileControlAffinity.leading,
          title: Text(title),
          subtitle: subtitle.isEmpty ? null : Text(subtitle),
          value: value,
          onChanged: onChanged == null ? null : (v) => onChanged(v == true),
        );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(translate(label)),
        ),
        if (_catalog == null)
          Padding(
            padding: const EdgeInsets.only(left: 8),
            child: Text(translate('Loading the Clanto service list…')),
          )
        else if (list.isEmpty && orphans.isEmpty)
          Padding(
            padding: const EdgeInsets.only(left: 8),
            child: Text(translate('No Clanto services available')),
          ),
        ...list.map((s) => tile(
            '${s.name} (${s.target})',
            s.description,
            enabled[s.id] == s.target,
            editable
                ? (v) => _toggleService(key, s.id, s.target, v)
                : null)),
        ...orphans.map((e) => tile(
            '${e.key} (${e.value})',
            translate('No longer in the Clanto service list'),
            true,
            editable
                ? (v) => _toggleService(key, e.key, e.value, v)
                : null)),
      ],
    );
  }

  /// macOS: attiva la Camera Extension (flutter/macos/Runner/AppDelegate.swift).
  Future<void> _activateMacCamera() async {
    String status;
    try {
      final res =
          await const MethodChannel('it.clanto.clantodesk/camera_extension')
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

  Widget _check(String label, String key, {bool bold = false}) {
    final value = _isY(key);
    return CheckboxListTile(
      dense: true,
      contentPadding: EdgeInsets.zero,
      controlAffinity: ListTileControlAffinity.leading,
      title: Text(translate(label),
          style: bold ? const TextStyle(fontWeight: FontWeight.bold) : null),
      value: value,
      onChanged: _editable(key)
          ? (v) async {
              await bind.mainSetOption(key: key, value: v == true ? 'Y' : 'N');
              if (mounted) setState(() {});
            }
          : null,
    );
  }

  Widget _indent(List<Widget> children) => Padding(
        padding: const EdgeInsets.only(left: 24),
        child: Column(
            crossAxisAlignment: CrossAxisAlignment.start, children: children),
      );

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
    final network = _isY(kOptClantoForwardEnabled);
    final webcam = _isY(kOptClantoForwardWebcam);
    // La camera virtuale di questo PC serve per ricevere una webcam: quella
    // del tecnico quando questo PC è controllato, quella del PC controllato
    // quando lo si usa da qui.
    final virtualCamera = webcam && !widget.controllerOnly;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _check('Enable network forwarding', kOptClantoForwardEnabled,
            bold: true),
        if (network)
          _indent([
            if (!widget.controllerOnly) ...[
              _services('Clanto services reachable on this PC',
                  kOptClantoForwardServicesHost, 'to_host'),
              _check('Allow custom forwards to this PC\'s network',
                  kOptClantoForwardCustomHost),
              if (_isY(kOptClantoForwardCustomHost))
                _targets('Allowed destinations on this PC\'s side',
                    kOptClantoForwardCustomHostTargets, _hostTargets),
              _check('Allow forwards towards the controller',
                  kOptClantoForwardReverse),
            ],
            _services('Clanto services of this PC for the controlled PC',
                kOptClantoForwardServicesController, 'to_controller'),
            _check(
                'Allow custom forwards from this PC\'s network as controller',
                kOptClantoForwardCustomController),
            if (_isY(kOptClantoForwardCustomController))
              _targets('Allowed destinations on the controller\'s side',
                  kOptClantoForwardCustomControllerTargets, _controllerTargets),
          ]),
        _check('Enable webcam forwarding', kOptClantoForwardWebcam, bold: true),
        if (webcam)
          _indent([
            _check('This PC\'s webcam to the controlled PC',
                kOptClantoForwardWebcamSend),
            if (_canReceiveWebcam)
              _check('The controlled PC\'s webcam to this PC',
                  kOptClantoForwardWebcamReceive),
            if (virtualCamera && Platform.isLinux)
              _targets('Virtual camera device (empty = automatic)',
                  kOptClantoForwardCameraDevice, _cameraDevice,
                  hint: '/dev/video10'),
            if (virtualCamera && Platform.isMacOS)
              Row(children: [
                TextButton(
                    onPressed: widget.enabled ? _activateMacCamera : null,
                    child: Text(translate('Activate the virtual camera'))),
                if (_macCameraStatus != null)
                  Flexible(child: Text(_macCameraStatus!)),
              ]),
          ]),
      ],
    );
  }
}

/// Voce "Forwarding" nelle impostazioni mobile (sezione Sicurezza).
SettingsTile clantoForwardingTile({bool enabled = true}) {
  return SettingsTile(
    title: Text(translate('Forwarding')),
    description: Text(translate(
        'Port forwarding and this phone\'s camera in the sessions you open.')),
    onPressed: enabled
        ? (context) => showDialog(
              context: context,
              builder: (context) => AlertDialog(
                content: ConstrainedBox(
                  constraints:
                      const BoxConstraints(maxWidth: 480, maxHeight: 600),
                  child: SingleChildScrollView(
                      child: ClantoForwardingSettings(
                          enabled: enabled, controllerOnly: true)),
                ),
                actions: [
                  TextButton(
                      onPressed: () => Navigator.of(context).pop(),
                      child: Text(translate('Close'))),
                ],
              ),
            )
        : null,
  );
}

/// Voci del menu della sessione: "Forwarding rete" e "Forwarding webcam",
/// ciascuna solo se attiva su questo PC.
List<TTextMenu> clantoForwardingMenus(BuildContext context, FFI ffi) {
  return [
    if (_isY(kOptClantoForwardEnabled))
      TTextMenu(
        child: Text(translate('Network forwarding')),
        onPressed: () => showDialog<void>(
          context: context,
          builder: (_) => _ForwardingDialog(sessionId: ffi.sessionId),
        ),
      ),
    if (_isY(kOptClantoForwardWebcam))
      TTextMenu(
        child: Text(translate('Webcam forwarding')),
        onPressed: () => showDialog<void>(
          context: context,
          builder: (_) => _WebcamDialog(sessionId: ffi.sessionId),
        ),
      ),
  ];
}

class _Service {
  final String id, name, direction, target, description;
  _Service(this.id, this.name, this.direction, this.target, this.description);
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

  @override
  void initState() {
    super.initState();
    _refresh();
    _timer = Timer.periodic(const Duration(seconds: 1), (_) => _refresh());
    _loadCatalog().then((list) {
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
    if (mounted) setState(() => _forwards = list);
  }

  Future<void> _add() async {
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
    final res = jsonDecode(await bind.sessionClantoForwardAdd(
      sessionId: widget.sessionId,
      direction: _direction,
      localPort: int.tryParse(_localPort.text.trim()) ?? 0,
      targetHost: host,
      targetPort: port,
      service: _serviceId,
    ));
    if (!mounted) return;
    setState(
        () => _error = res['error'] == null ? null : translate(res['error']));
    _refresh();
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
    // Verso il controller valgono solo i servizi spuntati su questo PC; verso
    // l'host decide il PC controllato.
    final mine = _enabledServices(kOptClantoForwardServicesController);
    final services = _catalog
        .where((s) =>
            s.direction == _direction &&
            (_direction == 'to_host' || mine[s.id] == s.target))
        .toList();
    return AlertDialog(
      title: Text(translate('Network forwarding')),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 560, maxHeight: 600),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
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
                      child: Text(translate(
                          'From this PC to the remote PC\'s network'))),
                  DropdownMenuItem(
                      value: 'to_controller',
                      child: Text(translate(
                          'From the remote PC to this PC\'s network'))),
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
                        value: '',
                        child: Text(translate('Custom destination'))),
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
                  child:
                      Text(_error!, style: const TextStyle(color: Colors.red)),
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

/// Forwarding webcam: dare la propria webcam al PC controllato, oppure usare
/// qui quella del PC controllato (con il consenso di chi è lì).
class _WebcamDialog extends StatefulWidget {
  final SessionID sessionId;
  const _WebcamDialog({required this.sessionId});

  @override
  State<_WebcamDialog> createState() => _WebcamDialogState();
}

enum _WebcamMode { send, receive }

class _WebcamDialogState extends State<_WebcamDialog> {
  final bool _canSend = _isY(kOptClantoForwardWebcamSend);
  final bool _canReceive =
      _canReceiveWebcam && _isY(kOptClantoForwardWebcamReceive);
  late _WebcamMode _mode =
      _canSend || !_canReceive ? _WebcamMode.send : _WebcamMode.receive;
  Timer? _timer;

  // Invio della propria webcam.
  List<Map<String, dynamic>> _cameras = [];
  int? _cameraIndex;
  Map<String, dynamic>? _sendStatus;
  String? _sendError;

  // Webcam del PC controllato.
  Map<String, dynamic>? _remote;
  int? _remoteIndex;
  String? _receiveError;

  @override
  void initState() {
    super.initState();
    if (_canSend) {
      bind.mainClantoCameras().then((json) {
        final res = jsonDecode(json);
        if (!mounted) return;
        setState(() {
          if (res['error'] != null) {
            _sendError = translate(res['error']);
          } else {
            _cameras = (res['ok'] as List).cast<Map<String, dynamic>>();
            _cameraIndex = _cameras.isEmpty ? null : _cameras.first['index'];
          }
        });
      });
    }
    _refresh();
    _timer = Timer.periodic(const Duration(seconds: 1), (_) => _refresh());
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  void _refresh() {
    final send =
        jsonDecode(bind.sessionClantoCameraStatus(sessionId: widget.sessionId));
    final remote = jsonDecode(
        bind.sessionClantoRemoteCameraStatus(sessionId: widget.sessionId));
    if (!mounted) return;
    setState(() {
      _sendStatus = send;
      _remote = remote;
      final cams = (_remote?['cameras'] as List?) ?? [];
      if (_remoteIndex == null && cams.isNotEmpty) {
        _remoteIndex = cams.first['index'];
      }
    });
  }

  // --- Invio ---

  bool get _sending =>
      _sendStatus != null &&
      ['starting', 'pending', 'active'].contains(_sendStatus!['state']);

  /// Canale e metodo di avvio della cattura nativa; null sul desktop.
  (MethodChannel, String)? get _nativeCamera {
    if (isAndroid) {
      return (const MethodChannel('mChannel'), 'clanto_camera_start');
    }
    if (isIOS) {
      return (const MethodChannel('it.clanto.clantodesk/camera'), 'start');
    }
    return null;
  }

  Future<void> _sendStart() async {
    final index = _cameraIndex;
    if (index == null) return;
    if (isAndroid &&
        !await AndroidPermissionManager.request('android.permission.CAMERA')) {
      if (mounted) {
        setState(() => _sendError = translate('Camera access denied'));
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
        error = await _nativeCamera?.$1
            .invokeMethod<String>(_nativeCamera!.$2, index == 1);
      } on PlatformException catch (e) {
        error = e.message ?? e.code;
      }
      if (error != null) {
        bind.sessionClantoCameraStop(sessionId: widget.sessionId);
      }
    }
    if (!mounted) return;
    setState(() => _sendError = error == null ? null : translate(error));
    _refresh();
  }

  void _sendStop() {
    bind.sessionClantoCameraStop(sessionId: widget.sessionId);
    if (isAndroid) {
      const MethodChannel('mChannel').invokeMethod('clanto_camera_stop');
    } else if (isIOS) {
      const MethodChannel('it.clanto.clantodesk/camera').invokeMethod('stop');
    }
    _refresh();
  }

  String? _statusText(Map<String, dynamic>? s) {
    if (s == null) return null;
    var text = translate(s['state'] ?? '');
    if (s['state'] == 'active') {
      text += ' ${s['width']}x${s['height']} → ${s['sink']}';
    }
    final message = (s['message'] ?? '') as String;
    if (message.isNotEmpty) text += ' ${translate(message)}';
    return text;
  }

  Widget _sendSection() {
    final status = _statusText(_sendStatus);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(translate(
            'Your webcam appears on the remote PC as «ClantoDesk Camera».')),
        const SizedBox(height: 8),
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
                onChanged:
                    _sending ? null : (v) => setState(() => _cameraIndex = v),
              ),
            ),
            const SizedBox(width: 8),
            _sending
                ? TextButton(
                    onPressed: _sendStop, child: Text(translate('Stop')))
                : TextButton(
                    onPressed: _sendStart, child: Text(translate('Start'))),
          ])
        else if (_sendError == null)
          Text(translate('No camera found')),
        if (status != null) Text(status),
        if (_sendError != null)
          Text(_sendError!, style: const TextStyle(color: Colors.red)),
      ],
    );
  }

  // --- Webcam del PC controllato ---

  Future<void> _request() async {
    final res = jsonDecode(
        await bind.sessionClantoCameraRequest(sessionId: widget.sessionId));
    if (!mounted) return;
    setState(() => _receiveError =
        res['error'] == null ? null : translate(res['error']));
    _refresh();
  }

  Future<void> _receiveStart() async {
    final index = _remoteIndex;
    if (index == null) return;
    final res = jsonDecode(await bind.sessionClantoRemoteCameraStart(
        sessionId: widget.sessionId, index: index));
    if (!mounted) return;
    setState(() => _receiveError =
        res['error'] == null ? null : translate(res['error']));
    _refresh();
  }

  void _receiveStop() {
    bind.sessionClantoRemoteCameraStop(sessionId: widget.sessionId);
    _refresh();
  }

  Widget _receiveSection() {
    final request = (_remote?['request'] ?? 'none') as String;
    final message = (_remote?['message'] ?? '') as String;
    final cams = ((_remote?['cameras'] as List?) ?? [])
        .cast<Map<String, dynamic>>();
    final receiver = _remote?['receiver'] as Map<String, dynamic>?;
    final receiving = receiver != null &&
        ['starting', 'active'].contains(receiver['state']);
    final children = <Widget>[
      Text(translate(
          'The remote PC\'s webcam appears on this PC as «ClantoDesk Camera».')),
      const SizedBox(height: 8),
    ];
    switch (request) {
      case 'pending':
        children.add(Text(
            translate('Waiting for the remote user to allow the webcam…')));
        break;
      case 'granted':
        if (cams.isEmpty) {
          children.add(Text(translate('No camera found')));
        } else {
          children.add(Row(children: [
            Expanded(
              child: DropdownButton<int>(
                value: _remoteIndex,
                isExpanded: true,
                items: cams
                    .map((cam) => DropdownMenuItem<int>(
                        value: cam['index'], child: Text(cam['name'] ?? '')))
                    .toList(),
                onChanged:
                    receiving ? null : (v) => setState(() => _remoteIndex = v),
              ),
            ),
            const SizedBox(width: 8),
            receiving
                ? TextButton(
                    onPressed: _receiveStop, child: Text(translate('Stop')))
                : TextButton(
                    onPressed: _receiveStart,
                    child: Text(translate('Start'))),
          ]));
        }
        break;
      default:
        children.add(TextButton(
            onPressed: _request,
            child: Text(translate('Ask to use the remote webcam'))));
        if (request == 'denied' && message.isNotEmpty) {
          children.add(Text(translate(message),
              style: const TextStyle(color: Colors.red)));
        }
    }
    final status = _statusText(receiver);
    if (status != null && request == 'granted') children.add(Text(status));
    if (request == 'granted' && message.isNotEmpty && !receiving) {
      children.add(
          Text(translate(message), style: const TextStyle(color: Colors.red)));
    }
    if (_receiveError != null) {
      children.add(Text(_receiveError!,
          style: const TextStyle(color: Colors.red)));
    }
    return Column(
        crossAxisAlignment: CrossAxisAlignment.start, children: children);
  }

  @override
  Widget build(BuildContext context) {
    Widget body;
    if (!_canSend && !_canReceive) {
      body = Text(translate(
          'Enable the webcam options in Settings, Forwarding, on this PC.'));
    } else {
      body = Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (_canSend)
            RadioListTile<_WebcamMode>(
              dense: true,
              contentPadding: EdgeInsets.zero,
              value: _WebcamMode.send,
              groupValue: _mode,
              title: Text(translate('Give my webcam to the remote PC')),
              onChanged: (v) => setState(() => _mode = v ?? _mode),
            ),
          if (_canReceive)
            RadioListTile<_WebcamMode>(
              dense: true,
              contentPadding: EdgeInsets.zero,
              value: _WebcamMode.receive,
              groupValue: _mode,
              title: Text(translate('Use the remote PC\'s webcam here')),
              onChanged: (v) => setState(() => _mode = v ?? _mode),
            ),
          const Divider(),
          _mode == _WebcamMode.send ? _sendSection() : _receiveSection(),
        ],
      );
    }
    return AlertDialog(
      title: Text(translate('Webcam forwarding')),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520, maxHeight: 600),
        child: SingleChildScrollView(child: body),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(translate('Close')),
        ),
      ],
    );
  }
}

/// Gestione connessioni dell'host: il tecnico chiede la webcam di questo PC.
void clantoCameraConsentDialog(int connId, String peer) {
  gFFI.dialogManager.show((setState, close, context) {
    void answer(bool accept) {
      bind.cmClantoCameraResponse(id: connId, accept: accept);
      close();
    }

    return CustomAlertDialog(
      title: Text(translate('Webcam request')),
      content: Text(
          '${translate('The technician asks to use the webcam of this PC.')}\n\n$peer'),
      actions: [
        dialogButton('Deny', onPressed: () => answer(false), isOutline: true),
        dialogButton('Allow', onPressed: () => answer(true)),
      ],
      onCancel: () => answer(false),
    );
  }, tag: 'clanto-camera-consent-$connId');
}
