// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Chiavi delle destinazioni di audit ClantoDesk.
//
// Gli eventi di connessione e trasferimento file nascono in
// src/server/connection.rs e vengono duplicati verso syslog e Windows Event Log
// da src/clanto/audit_sink.rs. L'interfaccia e' la card Audit di
// desktop_setting_page.dart.
//
// Le chiavi sono le stesse dichiarate in src/clanto/audit_sink.rs.
// Sono opzioni stringa con valore "Y", non booleane: lato Rust `option2bool`
// tratterebbe una chiave mai impostata come attiva.

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
