# ClantoDesk

ClantoDesk è la soluzione di desktop remoto e assistenza tecnica di
[Clanto Services](https://clanto.it), derivata dal progetto open source
[RustDesk](https://github.com/rustdesk/rustdesk).

Il progetto mantiene il motore multipiattaforma di RustDesk e aggiunge
branding, distribuzione, configurazione e controlli di sicurezza specifici per
l'infrastruttura Clanto. I branch di sviluppo seguono lo schema `clnt_X.Y.Z`;
`master` viene
mantenuto come mirror di upstream.

> ClantoDesk deve essere utilizzato esclusivamente su dispositivi propri o con
> l'autorizzazione del relativo proprietario. L'accesso non autorizzato a un
> dispositivo è vietato.

## Cosa permette di fare

- controllare da remoto computer Windows, macOS e Linux;
- collegarsi da smartphone e tablet Android o iOS a un computer;
- ricevere assistenza remota anche su Android;
- trasferire file in entrambe le direzioni;
- sincronizzare appunti e audio quando consentito;
- usare chat, terminale, inoltro porte e le altre funzioni supportate dal core;
- gestire sessioni, autorizzazioni e configurazioni centralizzate.

Su Android e iOS, i file da inviare vengono scelti con il selettore documenti
del sistema, che può mostrare anche cloud e provider esterni. Su Android i file
ricevuti vengono pubblicati nella cartella Download tramite MediaStore; su iOS
ClantoDesk apre l'app File e permette di scegliere una destinazione fra “Su
iPhone/iPad”, iCloud Drive e gli altri provider installati. Se l'esportazione
iOS viene annullata, la copia resta nella cartella Documents di ClantoDesk.
L'app non richiede accesso indiscriminato all'intera memoria del dispositivo.

## Differenze rispetto a RustDesk

ClantoDesk non è una semplice sostituzione del nome dell'applicazione. Il fork
include, tra le altre, queste personalizzazioni:

- identità applicativa `ClantoDesk` e bundle ID `it.clanto.clantodesk`;
- pacchetti e artefatti dedicati per Windows, macOS, Linux, Android e iOS;
- server rendezvous, relay e API configurati in fase di build;
- avvio *fail-closed* se una build distribuibile non contiene server e chiave
  pubblica Clanto validi;
- controllo CI contro ricadute accidentali sull'infrastruttura pubblica
  RustDesk;
- aggiornamenti applicativi tramite il servizio Clanto senza inviare il
  fingerprint hardware stabile del dispositivo;
- audit aggiuntivo verso syslog RFC 5424, via UDP o TCP, e Windows Event Log;
- trasferimento file mobile tramite picker di sistema, scoped storage e
  MediaStore su Android, ed esportazione documenti tramite app File su iOS;
- informativa e consenso espliciti prima dell'attivazione del servizio Android
  di Accessibilità;
- branding, icone, installer, servizi e metadati specifici ClantoDesk.

Le differenze operative e le trappole da conoscere prima di modificare il fork
sono documentate in [CLANTO.md](CLANTO.md).

## Sicurezza e privacy

Le build destinate alla distribuzione devono contenere `RENDEZVOUS_SERVER` e
`RS_PUB_KEY`. Se la configurazione manca o usa la chiave pubblica RustDesk,
ClantoDesk interrompe l'avvio invece di collegare silenziosamente l'utente a
un'infrastruttura diversa.

Per una build locale di sviluppo è possibile degradare questo controllo a un
avviso:

```bash
CLANTO_ALLOW_PUBLIC_SERVER=1 cargo build
```

Un binario compilato in questo modo non deve essere distribuito.

Su Android il servizio Accessibilità viene usato soltanto quando l'utente
sceglie di rendere il dispositivo controllabile da remoto. Serve a riprodurre
tocchi, scorrimenti e input da tastiera durante una sessione autorizzata; non è
usato per automazioni autonome. Prima di aprire le impostazioni Android,
l'applicazione mostra un'informativa separata e richiede un consenso esplicito.

- [Informativa privacy ClantoDesk](https://supporto.clanto.it/documentazione/privacy-clantodesk/)
- [Documentazione e supporto](https://supporto.clanto.it/documentazione/)

## Architettura

| Percorso | Contenuto |
| --- | --- |
| `src/` | Applicazione e core Rust |
| `src/server/` | Video, audio, input, clipboard e connessioni |
| `src/platform/` | Implementazioni specifiche dei sistemi operativi |
| `src/clanto/` | Funzionalità Rust specifiche ClantoDesk |
| `flutter/` | Interfaccia corrente desktop e mobile |
| `flutter/lib/clanto/` | Componenti Flutter specifici ClantoDesk |
| `flutter/android/.../clanto/` | Componenti Android specifici ClantoDesk |
| `flutter/ios/Runner/ClantoFileExporter.swift` | Esportazione file specifica iOS |
| `libs/hbb_common/` | Configurazione, protocolli e utilità condivise |
| `libs/scrap/` | Cattura dello schermo |
| `libs/enigo/` | Controllo di mouse e tastiera |
| `libs/clipboard/` | Clipboard e trasferimento tramite clipboard |
| `src/ui/` | Interfaccia Sciter precedente, deprecata |

La regola del fork è mantenere la logica Clanto nei moduli dedicati e inserire
nei file upstream soltanto hook minimi. Questo riduce i conflitti durante gli
aggiornamenti da RustDesk.

## Compilazione e verifica

Le build di release vengono prodotte dalle GitHub Actions del repository. I
workflow verificano preventivamente:

- presenza dei secret relativi ai server;
- coerenza della versione nei file di packaging;
- allineamento del submodule `libs/hbb_common`;
- assenza di artefatti distribuibili con nomi RustDesk.

Per un controllo locale del codice Rust, senza installare la toolchain sul
sistema host:

```bash
docker/check.sh
docker/check.sh --features flutter,linux-pkg-config
```

Questa modalità serve a verificare la compilazione. Le release ufficiali usano
la toolchain e il flusso definiti dalla CI.

## Configurazione di build

I principali valori iniettati durante la compilazione sono:

| Variabile | Scopo |
| --- | --- |
| `RENDEZVOUS_SERVER` | Server rendezvous Clanto |
| `RS_PUB_KEY` | Chiave pubblica del server |
| `API_SERVER` | API applicativa, quando configurata |
| `CLANTO_ALLOW_PUBLIC_SERVER` | Eccezione solo per sviluppo locale |

Non inserire secret, chiavi private o credenziali direttamente nel repository.

## Audit

Gli eventi di connessione e trasferimento file possono essere duplicati verso
destinazioni amministrative indipendenti dall'API server:

- syslog RFC 5424 su UDP o TCP;
- Windows Event Log, canale Application.

Le destinazioni sono disattivate per impostazione predefinita. Gli errori di
invio vengono registrati ma non interrompono né rallentano intenzionalmente la
sessione remota.

## Contributi e aggiornamenti upstream

Prima di modificare il progetto leggere [AGENTS.md](AGENTS.md) e
[CLANTO.md](CLANTO.md). In particolare:

- sviluppare sul branch di versione `clnt_X.Y.Z`, non su `master`;
- evitare refactoring non collegati alla modifica;
- mantenere `libs/hbb_common` allineato al submodule previsto;
- non sostituire manualmente `RustDesk` nei template di traduzione, negli
  script macOS o nei pattern del preprocessore MSI;
- preferire un modulo Clanto e un singolo hook rispetto a modifiche diffuse nei
  file upstream.

## Licenza e attribuzione

ClantoDesk è un'opera derivata da RustDesk. Il programma combinato è distribuito
secondo i termini della GNU Affero General Public License, versione 3 soltanto
(`AGPL-3.0-only`). Il testo ufficiale completo, mantenuto invariato rispetto a
upstream, è in [LICENCE](LICENCE).

I moduli separabili sviluppati interamente da Clanto Services srls e marcati
`AGPL-3.0-only OR Apache-2.0` possono essere usati, a scelta, anche secondo
Apache License 2.0. Questa concessione non si estende al codice RustDesk o di
terze parti e non cambia la licenza AGPL del fork nel suo complesso. L'elenco e
i criteri di classificazione sono in
[CLANTO-CONTRIBUTIONS.md](CLANTO-CONTRIBUTIONS.md); il testo Apache è in
[LICENSES/Apache-2.0.txt](LICENSES/Apache-2.0.txt).

- Progetto upstream: [rustdesk/rustdesk](https://github.com/rustdesk/rustdesk)
- Fork ClantoDesk: [clanto/clantodesk-client](https://github.com/clanto/clantodesk-client)
- Copyright delle modifiche ClantoDesk: Clanto Services srls

Le dipendenze e i componenti di terze parti conservano le rispettive licenze.
Le attribuzioni e la distinzione fra software, dipendenze e marchi sono
documentate in [NOTICE.md](NOTICE.md).

`ClantoDesk`, i loghi e gli elementi distintivi del brand appartengono a Clanto
Services srls. L'AGPL concede diritti di copyright sul software e sui file
distribuiti, ma non concede il diritto di usare il brand come marchio o di
suggerire che un fork sia un prodotto ufficiale ClantoDesk. Vedi
[TRADEMARKS.md](TRADEMARKS.md). Questa distinzione non aggiunge limitazioni ai
diritti sul software garantiti dall'AGPL.
