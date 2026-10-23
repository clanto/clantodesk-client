// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Controller fidati (host) e chiave di questo dispositivo (controller).
// La verifica sta in src/clanto/trusted_controllers.rs: qui si modifica solo
// l'opzione `clanto-trusted-controllers`, voci ID=CHIAVE separate da virgole.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:settings_ui/settings_ui.dart';

const String kOptClantoTrustedControllers = 'clanto-trusted-controllers';

final RegExp _idPattern = RegExp(r'^[A-Za-z0-9_-]{1,64}$');
final RegExp _keyPattern = RegExp(r'^[A-Za-z0-9_-]{43}$');

/// L'esenzione "Trust this device" di upstream e' sostituita da questo elenco.
bool clantoUpstreamTrustedDevicesVisible() => false;

String clantoDeviceKey() =>
    bind.mainGetCommonSync(key: 'clanto-device-key').trim();

class _TrustedController {
  final String id;
  final String key;

  const _TrustedController(this.id, this.key);

  @override
  String toString() => '$id=$key';
}

List<_TrustedController> _readTrustedControllers() {
  return bind
      .mainGetOptionSync(key: kOptClantoTrustedControllers)
      .split(RegExp(r'[,;\s]+'))
      .map((item) => item.split('='))
      .where((parts) =>
          parts.length == 2 &&
          _idPattern.hasMatch(parts[0]) &&
          _keyPattern.hasMatch(parts[1]))
      .map((parts) => _TrustedController(parts[0], parts[1]))
      .toList();
}

Future<void> showClantoTrustedControllersDialog(BuildContext context,
    {bool enabled = true}) {
  return showDialog<void>(
    context: context,
    builder: (context) => _TrustedControllersDialog(enabled: enabled),
  );
}

class _TrustedControllersDialog extends StatefulWidget {
  const _TrustedControllersDialog({required this.enabled});

  final bool enabled;

  @override
  State<_TrustedControllersDialog> createState() =>
      _TrustedControllersDialogState();
}

class _TrustedControllersDialogState extends State<_TrustedControllersDialog> {
  List<_TrustedController> _entries = _readTrustedControllers();
  final _id = TextEditingController();
  final _key = TextEditingController();
  String? _error;

  bool get _editable =>
      widget.enabled && !isOptionFixed(kOptClantoTrustedControllers);

  @override
  void dispose() {
    _id.dispose();
    _key.dispose();
    super.dispose();
  }

  Future<void> _save(List<_TrustedController> entries) async {
    await bind.mainSetOption(
        key: kOptClantoTrustedControllers, value: entries.join(','));
    if (mounted) {
      setState(() => _entries = _readTrustedControllers());
    }
  }

  Future<void> _add() async {
    final id = _id.text.trim();
    final key = _key.text.trim();
    if (!_idPattern.hasMatch(id)) {
      setState(() => _error = translate('Invalid controller ID'));
      return;
    }
    if (!_keyPattern.hasMatch(key)) {
      setState(() => _error = translate('Invalid device key'));
      return;
    }
    final entry = _TrustedController(id, key);
    _id.clear();
    _key.clear();
    setState(() => _error = null);
    if (_entries.any((e) => e.id == id && e.key == key)) return;
    await _save([..._entries, entry]);
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(translate('Trusted controllers')),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 560, maxHeight: 600),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(translate('trusted-controllers-tip'),
                  style: Theme.of(context).textTheme.bodySmall),
              const SizedBox(height: 12),
              if (_entries.isEmpty)
                Text(translate('No trusted controllers')),
              ..._entries.map((entry) => ListTile(
                    contentPadding: EdgeInsets.zero,
                    dense: true,
                    title: Text(entry.id),
                    subtitle: SelectableText(entry.key,
                        style: const TextStyle(fontFamily: 'monospace')),
                    trailing: IconButton(
                      tooltip: translate('Delete'),
                      icon: const Icon(Icons.delete_outline),
                      onPressed: _editable
                          ? () => _save(_entries
                              .where((e) => !(e.id == entry.id &&
                                  e.key == entry.key))
                              .toList())
                          : null,
                    ),
                  )),
              if (_editable) ...[
                const Divider(),
                TextField(
                  controller: _id,
                  decoration:
                      InputDecoration(labelText: translate('Controller ID')),
                ),
                TextField(
                  controller: _key,
                  decoration: InputDecoration(
                      labelText: translate('Device key'), errorText: _error),
                ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        if (_editable)
          TextButton(onPressed: _add, child: Text(translate('Add'))),
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(translate('Close')),
        ),
      ],
    );
  }
}

Future<void> showClantoDeviceKeyDialog(BuildContext context) {
  final key = clantoDeviceKey();
  return showDialog<void>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(translate('Device key')),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(translate('device-key-tip'),
                style: Theme.of(context).textTheme.bodySmall),
            const SizedBox(height: 12),
            key.isEmpty
                ? Text(translate('Device key unavailable'),
                    style: const TextStyle(color: Colors.red))
                : SelectableText(key,
                    style: const TextStyle(fontFamily: 'monospace')),
          ],
        ),
      ),
      actions: [
        if (key.isNotEmpty)
          TextButton(
            onPressed: () {
              Clipboard.setData(ClipboardData(text: key));
              showToast(translate('Copied'));
            },
            child: Text(translate('Copy')),
          ),
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(translate('Close')),
        ),
      ],
    ),
  );
}

/// Riga per la sezione 2FA della pagina impostazioni desktop.
Widget clantoTrustedControllersRow(BuildContext context,
    {bool enabled = true}) {
  return Align(
    alignment: Alignment.centerLeft,
    child: ElevatedButton(
      onPressed: () =>
          showClantoTrustedControllersDialog(context, enabled: enabled),
      child: Text(translate('Trusted controllers')),
    ),
  );
}

/// Pulsante per la scheda "Sicurezza in uscita" desktop.
Widget clantoDeviceKeyButton(BuildContext context) {
  return Align(
    alignment: Alignment.centerLeft,
    child: TextButton(
      onPressed: () => showClantoDeviceKeyDialog(context),
      child: Text(translate('Device key')),
    ),
  );
}

SettingsTile clantoTrustedControllersTile({bool enabled = true}) {
  return SettingsTile(
    title: Text(translate('Trusted controllers')),
    trailing: const Icon(Icons.arrow_forward_ios),
    onPressed: (context) =>
        showClantoTrustedControllersDialog(context, enabled: enabled),
  );
}

SettingsTile clantoDeviceKeyTile() {
  return SettingsTile(
    title: Text(translate('Device key')),
    onPressed: (context) => showClantoDeviceKeyDialog(context),
  );
}
