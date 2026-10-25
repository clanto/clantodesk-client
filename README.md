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
- usare chat testuale e chiamate vocali durante l'assistenza;
- usare terminale, inoltro porte e le altre funzioni supportate dal core;
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
- funzioni di sicurezza e amministrazione aggiuntive: accesso solo da
  dispositivi protetti, filigrane, controllo dei file ricevuti, permessi per ID,
  report sessioni, audit verso SIEM e criteri di gruppo (vedi
  [Sicurezza e amministrazione](#sicurezza-e-amministrazione));
- trasferimento file mobile tramite picker di sistema, scoped storage e
  MediaStore su Android, ed esportazione documenti tramite app File su iOS;
- informativa e consenso espliciti prima dell'attivazione del servizio Android
  di Accessibilità;
- icona flottante Android con branding ClantoDesk, badge dei messaggi non letti
  e accesso diretto alla chat;
- notifiche Android riconoscibili per nuovi messaggi e richieste di chiamata
  vocale, con apertura diretta della chat dalla relativa notifica;
- impronta crittografica presentata nell'interfaccia come “Impronta di
  sicurezza”, con uno stato esplicito quando non è ancora disponibile;
- configurazione dell'infrastruttura gestita e non modificabile dall'utente
  nelle build ufficiali;
- branding, icone, installer, servizi e metadati specifici ClantoDesk.

### Esperienza Android durante il controllo remoto

Quando un computer controlla un dispositivo Android, ClantoDesk mantiene
disponibile un'icona flottante con il proprio marchio. Se arrivano messaggi di
chat mentre l'app è in background, l'icona mostra il numero dei messaggi non
letti fino a `99+` e resta interamente visibile per non tagliare il badge.

Le notifiche chat funzionano anche nella direzione opposta: se il telefono sta
controllando un altro dispositivo e il tecnico passa temporaneamente al browser
o a un'altra app, ClantoDesk segnala i nuovi messaggi ricevuti dalla sessione.
Toccando la notifica viene ripristinata la sessione e aperta la relativa chat.
Con il dispositivo sbloccato la notifica mostra il titolo “Hai un nuovo
messaggio” e il relativo testo; sulla schermata bloccata viene mostrato soltanto
l'avviso generico, senza esporre il contenuto della conversazione.

Il menu dell'icona mostra `Chat (N)` quando ci sono messaggi da leggere. La voce
apre direttamente la scheda Chat e azzera il contatore. Lo stesso collegamento
diretto viene usato dalle notifiche dei nuovi messaggi. Entrando nella chat
dalla barra di navigazione dell'app, il conteggio viene sincronizzato e
azzerato allo stesso modo.

Le richieste di chiamata vocale usano una notifica distinta e chiaramente
identificata come chiamata, invece di una richiesta generica. Le notifiche
Android richiedono il relativo permesso nelle versioni del sistema che lo
prevedono.

Quando l'utente termina una sessione ricevuta, ClantoDesk distingue fra `Solo
disconnetti` e `Disconnetti e arresta`. La prima scelta chiude la connessione
corrente ma lascia attivo il servizio per l'accesso non presidiato: un
dispositivo autorizzato potrebbe quindi collegarsi nuovamente. La seconda
chiude tutte le sessioni e arresta il servizio, rendendo il telefono non
raggiungibile finché il servizio non viene riattivato.

Dopo `Solo disconnetti`, un avviso conferma che ClantoDesk resta disponibile e
offre l'azione immediata `Arresta ora`, così l'utente può cambiare decisione
senza cercare il comando nelle impostazioni.

L'arresto interrompe l'uso delle capacità di controllo da parte di ClantoDesk,
ma non revoca il permesso di Accessibilità nelle impostazioni di Android: per
ragioni di sicurezza del sistema operativo, tale autorizzazione può essere
revocata soltanto dall'utente o da una policy di gestione del dispositivo.

L'accesso alle impostazioni su Android e iOS è protetto dall'autenticazione
locale del dispositivo. Il sistema propone biometria quando disponibile e usa
PIN o passcode del dispositivo come metodo di recupero; ClantoDesk non riceve e
non conserva i dati biometrici o la credenziale di sblocco. L'autenticazione
viene richiesta nuovamente dopo che l'app è passata in background.

### Funzioni gestite e temporaneamente non esposte

Le build ufficiali ClantoDesk ricevono in fase di compilazione gli indirizzi
dei server rendezvous, relay e API e la chiave pubblica del server. Questi
valori sono bloccati e la schermata `ID/Relay Server` è nascosta su mobile e
desktop, per impedire che l'infrastruttura fidata venga sostituita per errore.
Gli indirizzi dei server e `RS_PUB_KEY` non sono credenziali segrete: la chiave
è pubblica e deve poter essere distribuita nel client.

Le sezioni seguenti restano implementate nel codice ma non sono esposte nelle
build distribuite finché i servizi Clanto corrispondenti non saranno pronti:

- `Account / Accedi`, che richiede un backend account compatibile;
- `Registra dispositivo` (deployment Android), che richiede API e token di
  registrazione emessi dal backend Clanto.

Nascondere queste sezioni evita di presentare percorsi di autenticazione non
utilizzabili e mantiene le normali funzioni dell'app accessibili senza un
account Clanto durante la revisione degli store.

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
| `libs/hbb_common/` | Configurazione, protocolli e utilità condivise con il server |
| `libs/base/` | Chiavi di opzione, protocollo `message.proto`, trasferimento file e codice di piattaforma usati solo dal client |
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

## Sicurezza e amministrazione

Le opzioni descritte qui riguardano soprattutto il PC **controllato** (host) e si
trovano nelle impostazioni di ClantoDesk. Il riferimento completo, con chiavi,
valori e limiti noti, è in [CLANTO.md](CLANTO.md).

### Sicurezza in ingresso

Con «Accetta connessioni solo da dispositivi aggiornati e protetti» (predefinito:
spento) l'host invia la postura dichiarata dal controller a
`https://clanto.it/api/clantodesk-posture.php` e rifiuta l'accesso se la risposta
non è `ok`. La verifica è **fail-closed**: API irraggiungibile, timeout di 5
secondi o risposta non valida valgono rifiuto.

- Le regole sono definite sul server e si modificano senza rilasciare il client.
- «Ignora questa regola su questo PC» aggira una singola regola su un singolo PC;
  ogni accesso entrato con un bypass viene registrato nell'audit.
- Con l'opzione attiva l'accesso diretto via IP è di fatto escluso.
- È una misura contro i dispositivi trascurati, non contro chi ricompila il client.

### Autenticazione a due fattori e controller fidati

Il controller non può chiedere di essere ricordato per saltare il 2FA: la casella
«Trust this device» di RustDesk non esiste. È l'amministratore
dell'host a decidere quali controller sono esentati, con l'elenco «Controller fidati»
nella sezione 2FA, con il criterio di gruppo `TrustedControllers` oppure da riga di
comando con privilegi di amministratore:

```bash
clantodesk --device-key                          # sul controller: stampa la chiave
clantodesk --trust-controller <ID> <CHIAVE>      # sull'host
clantodesk --untrust-controller <ID> [CHIAVE]    # sull'host
```

Ogni utente di ogni controller ha una chiave Ed25519 propria, visibile anche in
Impostazioni → Sicurezza → «Chiave del dispositivo». L'host salta il 2FA solo se il
controller firma la connessione con la chiave registrata per il suo ID: un ID
dichiarato, da solo, non basta. La password resta sempre obbligatoria.

### Filigrane

- **«Forza la filigrana del controller»**: l'host chiede al controller una filigrana
  diagonale con nome, ID, codice sessione, host e data. Con questa opzione l'host
  accetta solo il client ClantoDesk ufficiale, verificato da `clanto.it`, e disattiva
  il permesso di registrazione della sessione.
- **«Mostra la filigrana sull'host»**: una barra su ogni schermo del PC controllato con
  tecnico, ID, codice sessione e ora di inizio.

Limite residuo: resta possibile modificare un binario ufficiale conservandone la
chiave incorporata. La filigrana è un deterrente, non una protezione tecnica
contro la modifica del binario o contro acquisizioni con software o dispositivi
esterni. Le garanzie della barra sull'host variano per piattaforma (su Linux X11 può
comparire nell'immagine trasmessa; su Wayland il compositor può ignorarne
posizione e livello).

### Controllo dei file ricevuti

L'host può esaminare i file ricevuti dal controller, comprese le cartelle, con tre
modalità: scansione disattivata (predefinita), «Avvisa e consegna» e «Blocca i file
non sicuri». Nel blocco, file con malware e file non scansionabili vengono messi in
quarantena e il controller riceve un errore.

- **Windows:** AMSI, tramite qualunque antimalware registrato come provider.
- **macOS e Linux:** demone ClamAV `clamd`, via socket Unix o `host:porta`.
- **Android, iOS o motore non disponibile:** il file risulta non scansionabile.
- «Direzione trasferimento file» limita i trasferimenti a entrambe le direzioni, solo
  verso l'host, solo dall'host o nessuno; «Estensioni file bloccate» rifiuta i file in
  ingresso con le estensioni indicate.

Archivi protetti da password e contenuti non ispezionabili dal motore non sono
riconosciuti: per non consegnare file non verificati usare la modalità di blocco.

### Sicurezza in uscita

Le opzioni «Sicurezza in uscita» regolano cosa può fare **questo PC** quando
controlla altri dispositivi: trasferimento file e copia/incolla di file, appunti,
port forwarding e RDP, terminale, stampante remota e registrazione. Valgono solo
per ClantoDesk su questo PC e non impediscono di usare un altro programma.

### Permessi per ID autorizzato

Insieme all'elenco degli ID autorizzati, ogni ID o pattern (`*`, `?`) può avere un
profilo:

| Profilo | Effetto |
| --- | --- |
| Sola visualizzazione | Solo visione |
| Assistenza | Tastiera, appunti, file, audio, riavvio, blocco input, modalità privacy, stampante remota |
| Controllo completo | Tutti i permessi (profilo di una voce senza configurazione) |
| Personalizzato | Solo i permessi scelti |

Il profilo è sempre in AND con i permessi globali dell'host e non può ampliarli.
Una configurazione non valida applica la sola visualizzazione.

### Report sessioni

L'host conserva localmente uno storico delle sessioni in entrata, compresi i
tentativi rifiutati: orari, peer, IP, metodo di accesso, esito della postura,
permessi usati e metadati dei file. Non salva appunti, input da tastiera né altri
contenuti. Il report si filtra per periodo, esito, tipo di connessione e testo
(ID, nome, IP, codice sessione) ed è esportabile in CSV.

La conservazione predefinita è di 180 giorni; `0` disabilita il registro ed elimina
i file esistenti. I record contengono dati personali dei peer: conservarli per il
tempo strettamente necessario.

### Audit verso SIEM

Gli eventi di connessione, trasferimento file, allarmi, postura e controllo dei file
possono essere duplicati verso destinazioni indipendenti dall'API server.

- Syslog su UDP, TCP o TLS, con formato RFC 5424 o CEF. Il TLS usa solo la CA indicata
  nel file PEM configurato e, senza CA valida, non apre la connessione.
- Windows Event Log, canale Application.
- ID evento stabili, uguali in tutte le destinazioni: la tabella è in
  [CLANTO.md](CLANTO.md#integrazione-siem-dellaudit-host).

Le destinazioni sono disattivate per impostazione predefinita. UDP non è cifrato e
non ritenta; TCP e TLS usano una coda limitata a 1000 record. Gli errori di invio
vengono registrati e non interrompono la sessione remota.

### Credenziali e password temporanea

Segreti e credenziali locali sono cifrati con due chiavi indipendenti: **utente**
(password dei peer, rubrica, token dell'account), custodita in DPAPI, Keychain,
Secret Service o Android Keystore secondo la piattaforma; **macchina** (ID, password
permanente, chiave privata, dispositivi attendibili), in DPAPI macchina su Windows e
in un file con permessi ristretti altrove. La protezione macchina non difende da un
processo locale con privilegi amministrativi. Copiare i file di configurazione senza
la relativa chiave li rende indecifrabili.

L'opzione host `clanto-temp-password-rotate-minutes` ruota la password temporanea
ogni 1-1440 minuti; assente o `0`, nessuna rotazione.

### Criteri di gruppo

I criteri di gruppo (GPO/ADMX) si applicano **solo su Windows**: sulle altre
piattaforme `gpo::apply()` non fa nulla. I template ADMX/ADML (`en-US` e `it-IT`) sono
in `res/admx`: si copiano in `PolicyDefinitions`, nel Central Store del dominio o si
importano in Intune. I criteri sono letti da
`HKLM\Software\Policies\Clanto\ClantoDesk` **una sola volta all'avvio**: dopo
`gpupdate` riavviare il servizio. Le chiavi dei server non sono mai gestibili via GPO.
Il criterio `LockSettings` rende tutte le impostazioni di sola lettura, anche per gli
amministratori, tranne tema e lingua. La relativa chiave `clanto-lock-settings` è accettata
soltanto dalla GPO; i tentativi di scrittura da interfaccia, IPC o riga di comando sono rifiutati.

### Predefiniti più sicuri

- **PC controllato:** disconnessione per inattività dopo 10 minuti, scoperta LAN,
  tunnel TCP, riavvio remoto e terminale disattivati.
- **Dispositivi a cui ci si collega per la prima volta:** scala adattiva, appunti e audio disattivati,
  copia/incolla di file non consentito, cursore remoto visibile.

Un valore salvato dall'utente prevale sul predefinito.

### Opzioni di configurazione

Riferimento delle opzioni `clanto-*` e delle opzioni upstream con predefinito
modificato. Colonne: **Interfaccia** è l'etichetta italiana (sezione delle
impostazioni desktop tra parentesi), oppure «solo GPO o `--option`» se l'opzione non
ha controlli; **GPO** è il nome del criterio in `res/admx/clantodesk.admx`, «no» se la
chiave non è in `GPO_KEYS`. Dove non è indicato un valore predefinito esplicito,
«assente» significa che l'opzione non è impostata e vale il comportamento descritto.

**Impostarle senza interfaccia.** Su un'installazione, da amministratore:
`clantodesk --option <chiave> <valore>` scrive il valore, `clantodesk --option <chiave>`
lo legge. Senza installazione o senza privilegi risponde
`Installation and administrative privileges required!`; con le impostazioni
disabilitate risponde `Settings are disabled!`. Su Windows si può usare la GPO
(solo Windows): una chiave gestita da GPO non è sovrascrivibile dalle altre vie.
Con `clanto-lock-settings=Y`, una scrittura da riga di comando risponde
`Settings are managed by policy`.

#### Ingresso e postura

| Chiave | Valori | Default | Interfaccia | GPO |
| --- | --- | --- | --- | --- |
| `clanto-require-protected-device` | `Y`, `N` | `N` | «Accetta connessioni solo da dispositivi aggiornati e protetti» (Sicurezza in ingresso) | `RequireProtectedDevice` |
| `clanto-posture-bypass` | ID di regole separati da virgola | vuoto | «Regole di postura del dispositivo» → «Ignora questa regola su questo PC», pulsante «Modifica» attivo solo con la postura richiesta (Sicurezza in ingresso) | `PostureBypass` |

#### Filigrane

| Chiave | Valori | Default | Interfaccia | GPO |
| --- | --- | --- | --- | --- |
| `clanto-require-controller-watermark` | `Y`, `N` | assente (disattivata) | «Forza la filigrana del controller» (Sicurezza in ingresso) | `RequireControllerWatermark` |
| `clanto-host-watermark` | `Y`, `N` | `N` | «Mostra la filigrana sull'host» (Sicurezza in ingresso) | `HostWatermark` |

#### File

| Chiave | Valori | Default | Interfaccia | GPO |
| --- | --- | --- | --- | --- |
| `clanto-file-scan` | `off`, `warn`, `block` (altro valore = `off`) | `off` | «Scansione antivirus dei file ricevuti» (spunta = `block`) e «Modalità»: «Avvisa e consegna», «Blocca i file non sicuri» (Sicurezza in ingresso) | `FileScan` |
| `clanto-file-scan-clamd` | percorso socket Unix oppure `host:porta` | vuoto | «Endpoint ClamAV», non mostrato su Windows (Sicurezza in ingresso) | `FileScanClamd` |
| `clanto-file-scan-max-mb` | intero positivo, MB (GPO: 1-1048576) | `512` | «Dimensione massima file (MB)» (Sicurezza in ingresso) | `FileScanMaxMb` |
| `clanto-file-direction` | `both`, `to-host`, `from-host`, `none` | `both` | «Direzione trasferimento file» (Sicurezza in ingresso) | `FileDirection` |
| `clanto-file-blocked-ext` | estensioni separate da virgola, senza punti | vuoto | «Estensioni file bloccate» (Sicurezza in ingresso) | `FileBlockedExt` |

Le opzioni di scansione compaiono nell'interfaccia solo con la scansione attiva.

#### Uscita

Un valore diverso da `N` consente l'operazione.

| Chiave | Valori | Default | Interfaccia | GPO |
| --- | --- | --- | --- | --- |
| `clanto-outgoing-file-transfer` | `Y`, `N` | assente (consentito) | «Consenti trasferimento file e copia e incolla di file» (Sicurezza in uscita) | `OutgoingFileTransfer` |
| `clanto-outgoing-clipboard` | `Y`, `N` | assente (consentito) | «Consenti condivisione appunti» (Sicurezza in uscita) | `OutgoingClipboard` |
| `clanto-outgoing-tunnel` | `Y`, `N` | assente (consentito) | «Consenti port forwarding e RDP» (Sicurezza in uscita) | `OutgoingTunnel` |
| `clanto-outgoing-terminal` | `Y`, `N` | assente (consentito) | «Consenti terminale» (Sicurezza in uscita) | `OutgoingTerminal` |
| `clanto-outgoing-remote-printer` | `Y`, `N` | assente (consentito) | «Consenti stampante remota» (Sicurezza in uscita) | `OutgoingRemotePrinter` |
| `clanto-outgoing-record` | `Y`, `N` | assente (consentito) | «Consenti registrazione sessione» (Sicurezza in uscita) | `OutgoingRecord` |

#### Permessi per ID

| Chiave | Valori | Default | Interfaccia | GPO |
| --- | --- | --- | --- | --- |
| `clanto-id-permissions` | JSON: per voce `name`, `profile` (`view`, `support`, `full`, `custom`) e, per `custom`, `perms` | assente (ogni voce dell'elenco vale `full`; JSON non valido = `view`) | finestra «ID autorizzati» (Sicurezza) | `IdPermissions` |

L'elenco degli ID è l'opzione upstream `id-whitelist` (GPO `IdWhitelist`).

#### Controller fidati (2FA)

| Chiave | Valori | Default | Interfaccia | GPO |
| --- | --- | --- | --- | --- |
| `clanto-trusted-controllers` | voci `ID=CHIAVE` separate da virgole; la chiave è base64url di 43 caratteri | assente (nessuna esenzione) | «Controller fidati» (2FA) | `TrustedControllers` |

`enable-trusted-devices` di upstream è fissata a `N` e non è gestibile.

#### 2FA e privacy verso il server di gestione

| Chiave | Valori | Default | Interfaccia | GPO |
| --- | --- | --- | --- | --- |
| `clanto-require-2fa` | `Y`, `N` | `N` | solo GPO o `--option` (avviso nella sezione 2FA) | `RequireTwoFactor` |
| `clanto-share-device-identity` | `Y`, `N` | `N` (hostname e utente non inviati) | solo GPO o `--option` | `ShareDeviceIdentity` |

#### Forwarding

| Chiave | Valori | Default | Interfaccia | GPO |
| --- | --- | --- | --- | --- |
| `clanto-forward-enabled` | `Y`, `N` | `N` | «Abilita il forwarding» (Forwarding) | `ForwardEnabled` |
| `clanto-forward-catalog` | `Y`, `N` | acceso | «Usa il catalogo servizi Clanto» | `ForwardCatalog` |
| `clanto-forward-custom-host` | `Y`, `N` | `N` | «Consenti forward personalizzati verso la rete di questo PC» | `ForwardCustomHost` |
| `clanto-forward-custom-host-targets` | voci `indirizzo:porte` | vuoto | «Destinazioni ammesse dalla parte di questo PC» | `ForwardCustomHostTargets` |
| `clanto-forward-reverse` | `Y`, `N` | `N` | «Consenti forward verso il controller» | `ForwardReverse` |
| `clanto-forward-custom-controller` | `Y`, `N` | `N` | «Consenti forward personalizzati dalla rete di questo PC come controller» | `ForwardCustomController` |
| `clanto-forward-custom-controller-targets` | voci `indirizzo:porte` | vuoto | «Destinazioni ammesse dalla parte del controller» | `ForwardCustomControllerTargets` |
| `clanto-forward-camera` | `Y`, `N` | `N` | «Consenti la webcam del controller su questo PC» | `ForwardCamera` |
| `clanto-forward-camera-device` | percorso, es. `/dev/video10` (Linux) | vuoto = primo v4l2loopback | «Dispositivo della camera virtuale» | `ForwardCameraDevice` |

#### Report

| Chiave | Valori | Default | Interfaccia | GPO |
| --- | --- | --- | --- | --- |
| `clanto-session-log-days` | intero 0-3650 (`0` disabilita ed elimina i file; valore non numerico = 180) | `180` | «Conserva i report per (giorni)» nella sezione «Report sessioni» desktop e nel relativo dialogo mobile; `0` richiede conferma | `SessionLogDays` |

#### Audit e SIEM

L'interfaccia di questi controlli è nella sezione «Audit» delle impostazioni desktop.

| Chiave | Valori | Default | Interfaccia | GPO |
| --- | --- | --- | --- | --- |
| `clanto-syslog-enabled` | `Y` (altro valore = spento) | assente (spento) | «Invia gli eventi al SIEM» | `SyslogEnabled` |
| `clanto-syslog-host` | nome DNS o indirizzo del collettore | vuoto (senza host non si invia nulla) | «Host» | `SyslogHost` |
| `clanto-syslog-port` | 1-65535 | assente: `514` (UDP/TCP), `6514` (TLS) | «Porta» | `SyslogPort` |
| `clanto-syslog-protocol` | `udp`, `tcp`, `tls` | `tls` | «Protocollo» | `SyslogProtocol` |
| `clanto-syslog-facility` | 0-23 | `16` (`local0`) | «Facility» | `SyslogFacility` |
| `clanto-syslog-tls-ca` | percorso di un file PEM | vuoto (con TLS l'invio non parte) | «Percorso del certificato CA», solo con protocollo TLS | `SyslogTlsCa` |
| `clanto-syslog-format` | `rfc5424`, `cef` | `rfc5424` | «Formato» | `SyslogFormat` |
| `clanto-eventlog-enabled` | `Y` (altro valore = spento) | assente (spento) | «Scrivi gli eventi di audit nel registro eventi di Windows», solo su Windows | `EventLogEnabled` |

#### Password temporanea

| Chiave | Valori | Default | Interfaccia | GPO |
| --- | --- | --- | --- | --- |
| `clanto-temp-password-rotate-minutes` | intero 1-1440; `0` o vuoto = nessuna rotazione; altro valore = rotazione disattivata con avviso nel log | assente (nessuna rotazione) | menu «Ruota la password monouso» nella card Password desktop e nella sezione Sicurezza dell'host Android, visibile quando si usa la password temporanea | `TemporaryPasswordRotation` |

#### Interfaccia

| Chiave | Valori | Default | Interfaccia | GPO |
| --- | --- | --- | --- | --- |
| `clanto-privacy-prompt` | `Y`, `N` (`N` = mai il popup) | assente (popup attivo) | «Chiedi di attivare la modalità privacy all'inizio di ogni sessione» (Altre opzioni predefinite, desktop e mobile) | `PrivacyPrompt` |
| `clanto-lock-settings` | `Y`, `N` | assente (impostazioni modificabili) | nessun controllo locale; con `Y` tutte le impostazioni sono in sola lettura tranne tema e lingua | `LockSettings` |

#### Default dell'host

Valori applicati da `apply_host_defaults()`: un valore salvato dall'utente prevale. Le
prime sei sono opzioni upstream il cui **default è cambiato** nel fork. Le ultime
quattro sono descritte nelle tabelle delle rispettive aree.

| Chiave | Valori | Default ClantoDesk | Default upstream | Interfaccia | GPO |
| --- | --- | --- | --- | --- | --- |
| `allow-auto-disconnect` | `Y`, `N` | `Y` | `N` (solo `Y` attiva) | «Chiudi automaticamente sessioni in entrata per inattività utente» (Sicurezza) | `AllowAutoDisconnect` |
| `auto-disconnect-timeout` | minuti (GPO: 1-65535) | `10` | assente, che in `connection.rs` vale 10 minuti: effetto invariato | «Timeout in minuti» (Sicurezza) | `AutoDisconnectTimeout` |
| `enable-lan-discovery` | `Y`, `N` | `N` | assente (abilitato) | «Non effettuare rilevamento LAN», casella invertita (Sicurezza) | `EnableLanDiscovery` |
| `enable-tunnel` | `Y`, `N` | `N` | assente (abilitato) | «Abilita tunnel TCP» (Permessi) | `EnableTunnel` |
| `enable-remote-restart` | `Y`, `N` | `N` | assente (abilitato) | «Abilita riavvio da remoto» (Permessi) | `EnableRemoteRestart` |
| `enable-terminal` | `Y`, `N` | `N` | assente (abilitato) | «Abilita terminale» (Permessi) | `EnableTerminal` |
| `clanto-require-protected-device` | `Y`, `N` | `N` | — | vedi Ingresso e postura | `RequireProtectedDevice` |
| `clanto-posture-bypass` | ID separati da virgola | vuoto | — | vedi Ingresso e postura | `PostureBypass` |
| `clanto-session-log-days` | 0-3650 | `180` | — | vedi Report | `SessionLogDays` |
| `clanto-host-watermark` | `Y`, `N` | `N` | — | vedi Filigrane | `HostWatermark` |

Le opzioni `clanto-local-password`, `clanto-local-password-scope` e
`clanto-skip-privacy-prompt` (per singolo dispositivo) si impostano solo
dall'interfaccia e non sono gestibili via GPO.

### Requisiti di rete per le funzioni `clanto.it`

Postura e filigrana del controller richiedono che l'**host** raggiunga
`https://clanto.it/api/` e che i file della cartella `api/` del repository
(`clantodesk-posture.php`, `posture-policy.json`) siano pubblicati lì, con il
segreto di firma configurato sul server. Senza questi requisiti i login protetti
vengono rifiutati.

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
