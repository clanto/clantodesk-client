// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Fasce orarie di accesso all'host (src/clanto/access_hours.rs). Una fascia
// per giorno; un valore più complesso (es. da GPO) si modifica come testo.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';

const String kOptClantoAccessHours = 'clanto-access-hours';
const String _kDefaultHours = 'mon-fri 08:00-18:00';
const List<String> _kDays = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];
const List<String> _kDaysIt = ['lun', 'mar', 'mer', 'gio', 'ven', 'sab', 'dom'];
const List<String> _kDayLabels = [
  'Monday',
  'Tuesday',
  'Wednesday',
  'Thursday',
  'Friday',
  'Saturday',
  'Sunday'
];
final RegExp _kTime = RegExp(r'^([01]\d|2[0-3]):[0-5]\d$|^24:00$');

int? _day(String name) {
  final n = name.trim().toLowerCase();
  final i = _kDays.indexOf(n);
  return i >= 0 ? i : (_kDaysIt.contains(n) ? _kDaysIt.indexOf(n) : null);
}

/// Una fascia per giorno, oppure `null` se il valore non si rappresenta così.
Map<int, (String, String)>? _simpleSchedule(String value) {
  final out = <int, (String, String)>{};
  for (final entry in value.split(';').map((e) => e.trim())) {
    if (entry.isEmpty || entry.toLowerCase() == 'none') continue;
    final parts = entry.split(RegExp(r'\s+'));
    if (parts.length != 2 || parts[1].contains(',')) return null;
    final range = parts[1].split('-');
    if (range.length != 2) return null;
    final days = <int>[];
    for (final spec in parts[0].split(',')) {
      final ends = spec.split('-');
      final a = _day(ends.first), b = _day(ends.last);
      if (a == null || b == null || ends.length > 2) return null;
      for (var d = a;; d = (d + 1) % 7) {
        days.add(d);
        if (d == b) break;
      }
    }
    for (final d in days) {
      if (out.containsKey(d)) return null;
      out[d] = (range[0], range[1]);
    }
  }
  return out;
}

class ClantoAccessHoursSetting extends StatefulWidget {
  final bool enabled;
  const ClantoAccessHoursSetting({Key? key, this.enabled = true})
      : super(key: key);

  @override
  State<ClantoAccessHoursSetting> createState() =>
      _ClantoAccessHoursSettingState();
}

class _ClantoAccessHoursSettingState extends State<ClantoAccessHoursSetting> {
  String _value = bind.mainGetOptionSync(key: kOptClantoAccessHours);
  final _from = List.generate(7, (_) => TextEditingController());
  final _to = List.generate(7, (_) => TextEditingController());
  final _on = List.filled(7, false);
  final _raw = TextEditingController();
  bool _simple = true;

  @override
  void initState() {
    super.initState();
    _fill();
  }

  void _fill() {
    _raw.text = _value;
    final schedule = _simpleSchedule(_value);
    _simple = schedule != null;
    for (var d = 0; d < 7; d++) {
      final range = schedule?[d];
      _on[d] = range != null;
      _from[d].text = range?.$1 ?? '08:00';
      _to[d].text = range?.$2 ?? '18:00';
    }
  }

  @override
  void dispose() {
    for (final c in [..._from, ..._to, _raw]) {
      c.dispose();
    }
    super.dispose();
  }

  bool get _editable =>
      widget.enabled && !isOptionFixed(kOptClantoAccessHours);

  Future<void> _save(String value) async {
    await bind.mainSetOption(key: kOptClantoAccessHours, value: value);
    if (mounted) setState(() => _value = value);
  }

  Future<void> _saveDays() async {
    final entries = <String>[];
    for (var d = 0; d < 7; d++) {
      final from = _from[d].text.trim(), to = _to[d].text.trim();
      if (!_on[d]) continue;
      if (!_kTime.hasMatch(from) || !_kTime.hasMatch(to) || from == to) {
        return;
      }
      entries.add('${_kDays[d]} $from-$to');
    }
    // Nessun giorno spuntato: nessun accesso, non "sempre".
    await _save(entries.isEmpty ? 'none' : entries.join(';'));
  }

  Widget _dayRow(int d) {
    Widget time(TextEditingController c, String label) => SizedBox(
          width: 72,
          child: TextField(
            controller: c,
            enabled: _editable && _on[d],
            inputFormatters: [
              FilteringTextInputFormatter.allow(RegExp(r'[0-9:]')),
              LengthLimitingTextInputFormatter(5),
            ],
            decoration: InputDecoration(
                isDense: true, labelText: translate(label), hintText: 'HH:MM'),
            onSubmitted: (_) => _saveDays(),
            onTapOutside: (_) {
              if (_on[d]) _saveDays();
            },
          ),
        );
    return Row(children: [
      SizedBox(
        width: 160,
        child: CheckboxListTile(
          dense: true,
          contentPadding: EdgeInsets.zero,
          controlAffinity: ListTileControlAffinity.leading,
          title: Text(translate(_kDayLabels[d])),
          value: _on[d],
          onChanged: _editable
              ? (v) {
                  setState(() => _on[d] = v == true);
                  _saveDays();
                }
              : null,
        ),
      ),
      time(_from[d], 'From'),
      const SizedBox(width: 12),
      time(_to[d], 'To'),
    ]);
  }

  @override
  Widget build(BuildContext context) {
    final active = _value.trim().isNotEmpty;
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      CheckboxListTile(
        contentPadding: const EdgeInsets.symmetric(horizontal: 10),
        value: active,
        onChanged: _editable
            ? (v) async {
                await _save(v == true ? _kDefaultHours : '');
                setState(_fill);
              }
            : null,
        title: Text(translate('Allow connections only at set times')),
        subtitle: Text(translate(
            'Outside these hours every connection is rejected, in this PC\'s local time.')),
      ),
      if (active)
        Padding(
          padding: const EdgeInsets.only(left: 24, right: 10, bottom: 8),
          child: _simple
              ? Column(children: List.generate(7, _dayRow))
              : TextField(
                  controller: _raw,
                  enabled: _editable,
                  decoration: const InputDecoration(
                      hintText: 'mon-fri 08:00-13:00,14:00-18:00; sat 09:00-12:00'),
                  onSubmitted: (v) => _save(v.trim()),
                ),
        ),
    ]);
  }
}
