// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Impostazioni delle destinazioni di audit ClantoDesk.
//
// Gli eventi di connessione e trasferimento file nascono in
// src/server/connection.rs e vengono duplicati verso syslog e Windows Event Log
// da src/clanto/audit_sink.rs. Qui c'e' solo l'interfaccia per configurarli.
//
// Le chiavi sono le stesse dichiarate in src/clanto/audit_sink.rs.
// Sono opzioni stringa con valore "Y", non booleane: lato Rust `option2bool`
// tratterebbe una chiave mai impostata come attiva.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';
// marginOnly e' un'extension su Widget fornita da GetX, non da Flutter.
import 'package:get/get.dart';

const String kOptClantoSyslogEnabled = 'clanto-syslog-enabled';
const String kOptClantoSyslogHost = 'clanto-syslog-host';
const String kOptClantoSyslogPort = 'clanto-syslog-port';
const String kOptClantoSyslogProtocol = 'clanto-syslog-protocol';
const String kOptClantoSyslogFacility = 'clanto-syslog-facility';
const String kOptClantoSyslogTlsCa = 'clanto-syslog-tls-ca';
const String kOptClantoSyslogFormat = 'clanto-syslog-format';
const String kOptClantoEventLogEnabled = 'clanto-eventlog-enabled';

/// Default RFC 5424: local0. Deve restare allineato a DEFAULT_FACILITY in
/// src/clanto/audit_sink.rs.
const String kDefaultSyslogFacility = '16';

/// Righe da inserire in una _Card della pagina impostazioni.
List<Widget> clantoAuditRows(BuildContext context, {bool enabled = true}) {
  return [ClantoAuditSettings(enabled: enabled)];
}

class ClantoAuditSettings extends StatefulWidget {
  final bool enabled;
  const ClantoAuditSettings({Key? key, this.enabled = true}) : super(key: key);

  @override
  State<ClantoAuditSettings> createState() => _ClantoAuditSettingsState();
}

class _ClantoAuditSettingsState extends State<ClantoAuditSettings> {
  late bool _syslogOn;
  late bool _eventLogOn;
  late String _protocol;
  late String _format;
  final _host = TextEditingController();
  final _port = TextEditingController();
  final _facility = TextEditingController();
  final _tlsCa = TextEditingController();

  bool _isOn(String key) => bind.mainGetOptionSync(key: key) == 'Y';

  Future<void> _setOn(String key, bool on) async {
    await bind.mainSetOption(key: key, value: on ? 'Y' : '');
  }

  @override
  void initState() {
    super.initState();
    _syslogOn = _isOn(kOptClantoSyslogEnabled);
    _eventLogOn = _isOn(kOptClantoEventLogEnabled);
    _host.text = bind.mainGetOptionSync(key: kOptClantoSyslogHost);
    final proto = bind.mainGetOptionSync(key: kOptClantoSyslogProtocol);
    _protocol = const {'udp', 'tcp', 'tls'}.contains(proto.toLowerCase())
        ? proto.toLowerCase()
        : 'udp';
    final port = bind.mainGetOptionSync(key: kOptClantoSyslogPort);
    _port.text =
        port.isEmpty ? (_protocol == 'tls' ? '6514' : '514') : port;
    final format = bind.mainGetOptionSync(key: kOptClantoSyslogFormat);
    _format = const {'rfc5424', 'cef'}.contains(format.toLowerCase())
        ? format.toLowerCase()
        : 'rfc5424';
    final fac = bind.mainGetOptionSync(key: kOptClantoSyslogFacility);
    _facility.text = fac.isEmpty ? kDefaultSyslogFacility : fac;
    _tlsCa.text = bind.mainGetOptionSync(key: kOptClantoSyslogTlsCa);
  }

  @override
  void dispose() {
    _host.dispose();
    _port.dispose();
    _facility.dispose();
    _tlsCa.dispose();
    super.dispose();
  }

  Widget _checkbox(String label, bool value, ValueChanged<bool?>? onChanged) {
    return Row(children: [
      Checkbox(value: value, onChanged: onChanged).marginOnly(right: 5),
      Expanded(child: Text(translate(label))),
    ]);
  }

  Widget _field(String label, TextEditingController controller,
      {double width = 220,
      bool numeric = false,
      required String optionKey,
      required ValueChanged<String> onDone}) {
    return Row(children: [
      SizedBox(width: 90, child: Text(translate(label))),
      SizedBox(
        width: width,
        child: TextField(
          controller: controller,
          enabled: widget.enabled && _syslogOn && !isOptionFixed(optionKey),
          keyboardType: numeric ? TextInputType.number : TextInputType.text,
          inputFormatters:
              numeric ? [FilteringTextInputFormatter.digitsOnly] : null,
          decoration: const InputDecoration(isDense: true),
          onSubmitted: onDone,
          onTapOutside: (_) => onDone(controller.text),
        ),
      ),
    ]).marginOnly(left: 25, top: 8);
  }

  @override
  Widget build(BuildContext context) {
    final canEdit = widget.enabled;
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      _checkbox(
        'Send audit events to SIEM',
        _syslogOn,
        canEdit && !isOptionFixed(kOptClantoSyslogEnabled)
            ? (v) async {
                await _setOn(kOptClantoSyslogEnabled, v == true);
                setState(() => _syslogOn = v == true);
              }
            : null,
      ),
      if (_syslogOn) ...[
        _field('Host', _host,
            optionKey: kOptClantoSyslogHost,
            onDone: (v) =>
                bind.mainSetOption(key: kOptClantoSyslogHost, value: v.trim())),
        _field('Port', _port,
            width: 100,
            numeric: true,
            optionKey: kOptClantoSyslogPort,
            onDone: (v) {
          final p = int.tryParse(v.trim());
          bind.mainSetOption(
              key: kOptClantoSyslogPort,
              value: (p != null && p > 0 && p < 65536) ? '$p' : '514');
        }),
        Row(children: [
          SizedBox(width: 90, child: Text(translate('Protocol'))),
          DropdownButton<String>(
            value: _protocol,
            onChanged: canEdit && !isOptionFixed(kOptClantoSyslogProtocol)
                ? (v) async {
                    if (v == null) return;
                    await bind.mainSetOption(
                        key: kOptClantoSyslogProtocol, value: v);
                    setState(() => _protocol = v);
                  }
                : null,
            items: [
              DropdownMenuItem(
                  value: 'udp', child: Text(translate('UDP'))),
              DropdownMenuItem(
                  value: 'tcp', child: Text(translate('TCP'))),
              DropdownMenuItem(
                  value: 'tls', child: Text(translate('TLS'))),
            ],
          ),
        ]).marginOnly(left: 25, top: 8),
        Row(children: [
          SizedBox(width: 90, child: Text(translate('Format'))),
          DropdownButton<String>(
            value: _format,
            onChanged: canEdit && !isOptionFixed(kOptClantoSyslogFormat)
                ? (v) async {
                    if (v == null) return;
                    await bind.mainSetOption(
                        key: kOptClantoSyslogFormat, value: v);
                    setState(() => _format = v);
                  }
                : null,
            items: [
              DropdownMenuItem(
                  value: 'rfc5424', child: Text(translate('RFC 5424'))),
              DropdownMenuItem(
                  value: 'cef', child: Text(translate('CEF'))),
            ],
          ),
        ]).marginOnly(left: 25, top: 8),
        _field('Facility', _facility,
            width: 100,
            numeric: true,
            optionKey: kOptClantoSyslogFacility,
            onDone: (v) {
          final f = int.tryParse(v.trim());
          bind.mainSetOption(
              key: kOptClantoSyslogFacility,
              value: (f != null && f >= 0 && f <= 23)
                  ? '$f'
                  : kDefaultSyslogFacility);
        }),
        if (_protocol == 'tls')
          _field('CA certificate path', _tlsCa,
              optionKey: kOptClantoSyslogTlsCa,
              onDone: (v) => bind.mainSetOption(
                  key: kOptClantoSyslogTlsCa, value: v.trim())),
        if (_protocol == 'udp')
          Text(
            translate('UDP sends audit events without encryption.'),
            style: TextStyle(fontSize: 12, color: Theme.of(context).hintColor),
          ).marginOnly(left: 25, top: 4),
      ],
      if (isWindows)
        _checkbox(
          'Write audit events to Windows Event Log',
          _eventLogOn,
          canEdit && !isOptionFixed(kOptClantoEventLogEnabled)
              ? (v) async {
                  await _setOn(kOptClantoEventLogEnabled, v == true);
                  setState(() => _eventLogOn = v == true);
                }
              : null,
        ).marginOnly(top: 8),
    ]);
  }
}
