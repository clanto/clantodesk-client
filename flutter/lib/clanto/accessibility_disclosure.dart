// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Informativa specifica ClantoDesk mostrata prima di aprire le impostazioni
// AccessibilityService. Deve restare separata dalle altre informative.

import 'dart:ui';

class AccessibilityDisclosureCopy {
  const AccessibilityDisclosureCopy({
    required this.title,
    required this.body,
    required this.decline,
    required this.accept,
  });

  final String title;
  final String body;
  final String decline;
  final String accept;
}

AccessibilityDisclosureCopy clantoAccessibilityDisclosure() {
  final language = PlatformDispatcher.instance.locale.languageCode;
  if (language == 'it') {
    return const AccessibilityDisclosureCopy(
      title: 'Consenso al controllo remoto',
      body: 'ClantoDesk utilizza il servizio Accessibilità esclusivamente per '
          'consentire a un operatore remoto da te autorizzato di controllare '
          'questo dispositivo. Durante una sessione il servizio può accedere '
          'alle finestre attive e al contenuto dei campi di testo modificabili '
          'per riprodurre tocchi, scorrimenti e inserimenti da tastiera.\n\n'
          'Queste informazioni vengono elaborate sul dispositivo soltanto per '
          'eseguire i comandi della sessione: il servizio Accessibilità non le '
          'salva e non le invia autonomamente. L’operatore autorizzato può '
          'comunque vedere lo schermo condiviso durante la sessione remota.\n\n'
          'Il servizio non esegue automazioni autonome. Puoi rifiutare ora, '
          'interrompere la sessione e disattivare ClantoDesk Input in qualsiasi '
          'momento dalle impostazioni Android.',
      decline: 'Non accetto',
      accept: 'Accetto e apri Impostazioni',
    );
  }
  return const AccessibilityDisclosureCopy(
    title: 'Consent to remote control',
    body: 'ClantoDesk uses the Accessibility service solely to let a remote '
        'operator authorized by you control this device. During a session, the '
        'service may access active windows and editable text fields to perform '
        'taps, scrolling, and keyboard input.\n\n'
        'This information is processed on the device only to carry out session '
        'commands: the Accessibility service does not store it or send it on '
        'its own. The authorized operator may still see the screen shared '
        'during the remote session.\n\n'
        'The service does not perform autonomous automation. You may decline '
        'now, end the session, and disable ClantoDesk Input at any time in '
        'Android settings.',
    decline: 'Decline',
    accept: 'Accept and open Settings',
  );
}
