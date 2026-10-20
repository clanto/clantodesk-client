// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

import 'package:flutter/material.dart';
import 'package:flutter_hbb/clanto/id_permissions_data.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/models/platform_model.dart';

export 'id_permissions_data.dart';

const String kOptClantoIdPermissions = 'clanto-id-permissions';

Future<bool> showClantoIdPermissionsDialog(
  BuildContext context, {
  required bool enabled,
}) async {
  final whitelist = bind.mainGetOptionSync(key: kOptionIdWhitelist);
  final permissions = bind.mainGetOptionSync(key: kOptClantoIdPermissions);
  return await showDialog<bool>(
        context: context,
        builder: (context) => _IdPermissionsDialog(
          entries: clantoParseIdPermissions(
            permissions,
            clantoParseIdWhitelist(whitelist),
          ).entries,
          enabled: enabled &&
              !isOptionFixed(kOptionIdWhitelist) &&
              !isOptionFixed(kOptClantoIdPermissions),
        ),
      ) ??
      false;
}

class _IdPermissionsDialog extends StatefulWidget {
  const _IdPermissionsDialog({
    required this.entries,
    required this.enabled,
  });

  final List<ClantoIdPermissionEntry> entries;
  final bool enabled;

  @override
  State<_IdPermissionsDialog> createState() => _IdPermissionsDialogState();
}

class _IdPermissionsDialogState extends State<_IdPermissionsDialog> {
  late final List<_IdPermissionDraft> _entries;
  String _error = '';
  bool _saving = false;

  @override
  void initState() {
    super.initState();
    _entries = widget.entries.map(_IdPermissionDraft.fromEntry).toList();
  }

  @override
  void dispose() {
    for (final entry in _entries) {
      entry.dispose();
    }
    super.dispose();
  }

  String _profileLabel(ClantoIdProfile profile) {
    return switch (profile) {
      ClantoIdProfile.view => translate('View only'),
      ClantoIdProfile.support => translate('Support'),
      ClantoIdProfile.full => translate('Full control'),
      ClantoIdProfile.custom => translate('Custom'),
    };
  }

  String _permissionLabel(String permission) {
    return translate(switch (permission) {
      'keyboard' => 'Enable keyboard/mouse',
      'clipboard' => 'Enable clipboard',
      'file' => 'Enable file transfer',
      'audio' => 'Enable audio',
      'camera' => 'Enable camera',
      'terminal' => 'Enable terminal',
      'tunnel' => 'Enable TCP tunneling',
      'restart' => 'Enable remote restart',
      'recording' => 'Enable recording session',
      'block_input' => 'Enable blocking user input',
      'privacy_mode' => 'Enable privacy mode',
      'remote_printer' => 'Enable remote printer',
      _ => permission,
    });
  }

  Future<void> _editPermissions(_IdPermissionDraft entry) async {
    final selected = Set<String>.of(entry.permissions);
    final result = await showDialog<Set<String>>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) => AlertDialog(
          title: Text(translate('Permissions')),
          content: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 440, maxHeight: 560),
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: clantoIdPermissionKeys
                    .map((permission) => CheckboxListTile(
                          contentPadding: EdgeInsets.zero,
                          value: selected.contains(permission),
                          onChanged: (value) => setDialogState(() {
                            if (value == true) {
                              selected.add(permission);
                            } else {
                              selected.remove(permission);
                            }
                          }),
                          title: Text(_permissionLabel(permission)),
                        ))
                    .toList(growable: false),
              ),
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: Text(translate('Cancel')),
            ),
            TextButton(
              onPressed: () => Navigator.of(context).pop(selected),
              child: Text(translate('OK')),
            ),
          ],
        ),
      ),
    );
    if (result != null && mounted) {
      setState(() => entry.permissions = result);
    }
  }

  void _addEntry() {
    setState(() {
      _entries.add(_IdPermissionDraft.empty());
      _error = '';
    });
  }

  void _removeEntry(int index) {
    setState(() {
      _entries.removeAt(index).dispose();
      _error = '';
    });
  }

  Future<void> _save() async {
    final entries = <ClantoIdPermissionEntry>[];
    final patterns = <String>{};
    for (final draft in _entries) {
      final pattern = draft.pattern.text.trim();
      if (pattern.isEmpty) {
        setState(() => _error = translate('ID or pattern is required'));
        return;
      }
      final invalid = pattern.runes.any(
            (char) => char <= 0x20 || (char >= 0x7f && char <= 0x9f),
          ) ||
          pattern.contains(',') ||
          pattern.contains(';');
      if (invalid) {
        setState(() => _error = '${translate('Invalid ID')} $pattern');
        return;
      }
      if (!patterns.add(pattern.toLowerCase())) {
        setState(() => _error = translate('Duplicate ID or pattern'));
        return;
      }
      entries.add(draft.toEntry(pattern));
    }

    setState(() {
      _saving = true;
      _error = '';
    });
    final whitelist = clantoSerializeIdWhitelist(entries);
    await bind.mainSetOption(
      key: kOptClantoIdPermissions,
      value: clantoSerializeIdPermissions(entries),
    );
    await bind.mainSetOption(
      key: kOptionIdWhitelist,
      value: whitelist.isEmpty ? defaultOptionWhitelist : whitelist,
    );
    if (mounted) Navigator.of(context).pop(true);
  }

  Widget _profileField(_IdPermissionDraft entry) {
    return DropdownButtonFormField<ClantoIdProfile>(
      value: entry.profile,
      isExpanded: true,
      decoration: const InputDecoration(isDense: true),
      items: ClantoIdProfile.values
          .map((profile) => DropdownMenuItem(
                value: profile,
                child: Text(_profileLabel(profile)),
              ))
          .toList(growable: false),
      onChanged: widget.enabled
          ? (profile) {
              if (profile != null) setState(() => entry.profile = profile);
            }
          : null,
    );
  }

  Widget _actions(_IdPermissionDraft entry, int index) {
    return Wrap(
      spacing: 4,
      children: [
        if (entry.profile == ClantoIdProfile.custom)
          TextButton(
            onPressed: widget.enabled ? () => _editPermissions(entry) : null,
            child: Text(translate('Modify')),
          ),
        IconButton(
          tooltip: translate('Remove'),
          onPressed: widget.enabled ? () => _removeEntry(index) : null,
          icon: const Icon(Icons.delete_outline),
        ),
      ],
    );
  }

  Widget _desktopTable() {
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: DataTable(
        columns: [
          DataColumn(label: Text(translate('ID or pattern'))),
          DataColumn(label: Text(translate('Name'))),
          DataColumn(label: Text(translate('Profile'))),
          const DataColumn(label: SizedBox.shrink()),
        ],
        rows: _entries.indexed
            .map((item) => DataRow(cells: [
                  DataCell(SizedBox(
                    width: 190,
                    child: TextField(
                      controller: item.$2.pattern,
                      enabled: widget.enabled,
                      decoration: const InputDecoration(isDense: true),
                    ).workaroundFreezeLinuxMint(),
                  )),
                  DataCell(SizedBox(
                    width: 150,
                    child: TextField(
                      controller: item.$2.name,
                      enabled: widget.enabled,
                      decoration: const InputDecoration(isDense: true),
                    ).workaroundFreezeLinuxMint(),
                  )),
                  DataCell(SizedBox(
                    width: 175,
                    child: _profileField(item.$2),
                  )),
                  DataCell(_actions(item.$2, item.$1)),
                ]))
            .toList(growable: false),
      ),
    );
  }

  Widget _mobileList() {
    return Column(
      children: _entries.indexed
          .map((item) => Card(
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Column(children: [
                    TextField(
                      controller: item.$2.pattern,
                      enabled: widget.enabled,
                      decoration: InputDecoration(
                        isDense: true,
                        labelText: translate('ID or pattern'),
                      ),
                    ).workaroundFreezeLinuxMint(),
                    const SizedBox(height: 10),
                    TextField(
                      controller: item.$2.name,
                      enabled: widget.enabled,
                      decoration: InputDecoration(
                        isDense: true,
                        labelText: translate('Name'),
                      ),
                    ).workaroundFreezeLinuxMint(),
                    const SizedBox(height: 10),
                    Row(children: [
                      Expanded(child: _profileField(item.$2)),
                      _actions(item.$2, item.$1),
                    ]),
                  ]),
                ),
              ))
          .toList(growable: false),
    );
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(translate('Authorized IDs')),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 820, maxHeight: 620),
        child: SizedBox(
          width: 820,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(translate('id_whitelist_wildcard_tip')),
              const SizedBox(height: 8),
              Text(translate('id_whitelist_caveat_tip')),
              if (_error.isNotEmpty) ...[
                const SizedBox(height: 8),
                Text(_error, style: const TextStyle(color: Colors.red)),
              ],
              const SizedBox(height: 8),
              Flexible(
                child: SingleChildScrollView(
                  child: LayoutBuilder(
                    builder: (context, constraints) => constraints.maxWidth < 680
                        ? _mobileList()
                        : _desktopTable(),
                  ),
                ),
              ),
              if (widget.enabled)
                TextButton.icon(
                  onPressed: _addEntry,
                  icon: const Icon(Icons.add),
                  label: Text(translate('Add ID')),
                ),
              if (_saving) const LinearProgressIndicator(),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: _saving ? null : () => Navigator.of(context).pop(),
          child: Text(translate(widget.enabled ? 'Cancel' : 'Close')),
        ),
        if (widget.enabled)
          TextButton(
            onPressed: _saving ? null : _save,
            child: Text(translate('OK')),
          ),
      ],
    );
  }
}

class _IdPermissionDraft {
  _IdPermissionDraft({
    required String pattern,
    required String name,
    required this.profile,
    required this.permissions,
  })  : pattern = TextEditingController(text: pattern),
        name = TextEditingController(text: name);

  factory _IdPermissionDraft.fromEntry(ClantoIdPermissionEntry entry) {
    return _IdPermissionDraft(
      pattern: entry.pattern,
      name: entry.name,
      profile: entry.profile,
      permissions: Set.of(entry.permissions),
    );
  }

  factory _IdPermissionDraft.empty() => _IdPermissionDraft(
        pattern: '',
        name: '',
        profile: ClantoIdProfile.full,
        permissions: {},
      );

  final TextEditingController pattern;
  final TextEditingController name;
  ClantoIdProfile profile;
  Set<String> permissions;

  ClantoIdPermissionEntry toEntry(String normalizedPattern) {
    return ClantoIdPermissionEntry(
      pattern: normalizedPattern,
      name: name.text.trim(),
      profile: profile,
      permissions: Set.unmodifiable(permissions),
    );
  }

  void dispose() {
    pattern.dispose();
    name.dispose();
  }
}
