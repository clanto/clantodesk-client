// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Popup "Vuoi attivare la modalita' privacy?" all'ingresso in un PC.
//
// Agganciato da una sola riga in ImageModel/FFI.onEvent2UIRgba (primo frame):
// a quel punto peer info, permessi e platform additions sono gia' arrivati,
// quindi la decisione coincide con quella della toolbar.

import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/common/shared_state.dart';
import 'package:flutter_hbb/common/widgets/toolbar.dart';
import 'package:flutter_hbb/models/model.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:get/get.dart';
import 'package:settings_ui/settings_ui.dart';

/// Opzione per dispositivo (`ui_flutter` del peer): "Y" = non chiedere piu'.
const String kOptClantoSkipPrivacyPrompt = 'clanto-skip-privacy-prompt';

/// Opzione globale di `Config` (ClantoDesk2.toml, [options]): "N" = mai il popup,
/// assente o "Y" = attivo. Impostabile anche con `clantodesk --option <chiave> N`.
const String kOptClantoPrivacyPrompt = 'clanto-privacy-prompt';

bool clantoPrivacyPromptEnabled() =>
    bind.mainGetOptionSync(key: kOptClantoPrivacyPrompt) != 'N';

Future<void> _setPrivacyPromptEnabled(bool on) =>
    bind.mainSetOption(key: kOptClantoPrivacyPrompt, value: on ? 'Y' : 'N');

/// Riga per la scheda "Other Default Options" delle impostazioni desktop.
class ClantoPrivacyPromptSetting extends StatefulWidget {
  const ClantoPrivacyPromptSetting({Key? key}) : super(key: key);

  @override
  State<ClantoPrivacyPromptSetting> createState() =>
      _ClantoPrivacyPromptSettingState();
}

class _ClantoPrivacyPromptSettingState
    extends State<ClantoPrivacyPromptSetting> {
  bool _on = clantoPrivacyPromptEnabled();

  Future<void> _toggle() async {
    await _setPrivacyPromptEnabled(!_on);
    setState(() => _on = clantoPrivacyPromptEnabled());
  }

  @override
  Widget build(BuildContext context) {
    final enabled = !isOptionFixed(kOptClantoPrivacyPrompt);
    return GestureDetector(
      onTap: enabled ? _toggle : null,
      child: Row(children: [
        Checkbox(value: _on, onChanged: enabled ? (_) => _toggle() : null),
        Expanded(
            child: Text(translate(
                'Ask to enable privacy mode at the start of every session'))),
      ]).marginOnly(left: 10),
    );
  }
}

/// Tile per le impostazioni mobile; `refresh` ridisegna la pagina.
SettingsTile clantoPrivacyPromptTile(VoidCallback refresh) {
  return SettingsTile.switchTile(
    initialValue: clantoPrivacyPromptEnabled(),
    title: Text(translate(
        'Ask to enable privacy mode at the start of every session')),
    onToggle: isOptionFixed(kOptClantoPrivacyPrompt)
        ? null
        : (v) async {
      await _setPrivacyPromptEnabled(v);
      refresh();
    },
  );
}

class ClantoPrivacyPrompt {
  // Chiave: il PeerInfo della sessione. clear() lo sostituisce, quindi una
  // riconnessione automatica non ripropone il popup e una nuova sessione si'.
  static final Expando<bool> _asked = Expando<bool>();

  static Future<void> onFirstImage(FFI ffi) async {
    try {
      await _maybeShow(ffi);
    } catch (e) {
      debugPrint('ClantoPrivacyPrompt: $e');
    }
  }

  static Future<void> _maybeShow(FFI ffi) async {
    if (isWeb || ffi.connType != ConnType.defaultConn) return;
    if (!clantoPrivacyPromptEnabled()) return;
    if (bind.isIncomingOnly()) return;
    final model = ffi.ffiModel;
    final pi = model.pi;
    if (_asked[pi] == true) return;
    _asked[pi] = true;
    if (model.viewOnly || !model.keyboard) return;
    final state = PrivacyModeState.find(ffi.id);
    if (state.isNotEmpty) return;
    final skip = await bind.sessionGetFlutterOption(
        sessionId: ffi.sessionId, k: kOptClantoSkipPrivacyPrompt);
    final ctx = globalKey.currentContext;
    if (skip == 'Y' || ffi.closed || ctx == null) return;

    // Stessa lista della toolbar: vuota se il peer non supporta la modalita'
    // privacy o manca il permesso. Il primo elemento e' il click singolo.
    final menus = toolbarPrivacyMode(state, ctx, ffi.id, ffi);
    final supported = pi.features.privacyMode && menus.isNotEmpty;
    _showDialog(ffi, supported, () => menus.first.onChanged?.call(true));
  }

  static void _showDialog(FFI ffi, bool supported, VoidCallback enable) {
    bool dontAsk = false;
    ffi.dialogManager.show((setState, close, context) {
      void finish(bool turnOn) {
        if (dontAsk) {
          bind.sessionSetFlutterOption(
              sessionId: ffi.sessionId, k: kOptClantoSkipPrivacyPrompt, v: 'Y');
        }
        close();
        if (turnOn) enable();
      }

      return CustomAlertDialog(
        title: Text(translate('Privacy mode')),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(translate(supported
                ? 'Enable privacy mode on this device?'
                : 'Privacy mode is not available on this device.')),
            const SizedBox(height: 8),
            InkWell(
              onTap: () => setState(() => dontAsk = !dontAsk),
              child: Row(children: [
                Checkbox(
                    value: dontAsk,
                    onChanged: (v) => setState(() => dontAsk = v == true)),
                Expanded(
                    child: Text(translate("Don't ask again for this device"))),
              ]),
            ),
          ],
        ),
        actions: [
          if (supported)
            dialogButton('Not now',
                isOutline: true, onPressed: () => finish(false)),
          dialogButton(supported ? 'Turn on' : 'OK',
              onPressed: () => finish(supported)),
        ],
        onSubmit: () => finish(supported),
        onCancel: () => finish(false),
      );
    });
  }
}
