# Note sul fork ClantoDesk

Questo repo è un fork di [rustdesk/rustdesk](https://github.com/rustdesk/rustdesk).
I branch di lavoro seguono lo schema `clnt_X.Y.Z`. `master` è un mirror di
upstream, non ci si sviluppa.

Qui stanno solo le cose che **non si capiscono leggendo il codice** e che portano a
fare la modifica sbagliata. Per il resto vale la documentazione upstream (`AGENTS.md`).

## Trappole da conoscere prima di modificare

### 1. Le traduzioni sono template: non tradurre "RustDesk"

`src/lang/*.rs` contiene ~600 occorrenze di "RustDesk". Sono corrette così.
`translate()` in [src/lang.rs](src/lang.rs) le sostituisce a runtime con
`get_app_name()`, cioè "ClantoDesk", perché `APP_NAME` è impostato in
`global_init()`. Modificarle a mano non cambia nulla per l'utente e crea
conflitti a ogni merge upstream.

Due eccezioni volute, non sostituite: le chiavi `upgrade_rustdesk_server_pro*` e
`powered_by_me`, che si riferiscono al prodotto RustDesk vero.

### 2. Gli script macOS di installazione sono template

`src/platform/privileges_scripts/*.plist` e `*.scpt` contengono
`com.carriez.rustdesk`, `rustdesk`, `RustDesk`. Sono **segnaposto**:
`correct_app_name()` in [src/platform/macos.rs](src/platform/macos.rs) li
sostituisce a runtime con il bundle id e il nome app reali. Cambiarli rompe la
sostituzione.

Corollario: se il bundle id è sbagliato, `get_bundle_id()` restituisce il valore
upstream e la sostituzione diventa un no-op silenzioso. È già capitato.

### 3. `res/msi/preprocess.py` è il motore di branding, non un file da brandizzare

Le stringhe "RustDesk" al suo interno sono il **pattern di ricerca**
(`line.replace("RustDesk", app_name)`). Due parametri distinti, e vanno tenuti
separati:

- `--app-name clantodesk` — deve corrispondere al nome dell'eseguibile, e da esso
  è derivato l'**UpgradeCode** (`uuid5(NAMESPACE_OID, app_name + ".exe")`).
  Cambiarlo rompe l'aggiornamento delle installazioni esistenti.
- `--display-name ClantoDesk` — solo per i nomi visibili (installer, *App installate*).

`update_license_file()` esce senza fare nulla se `License.rtf` non contiene
"Purslane": serve a non distruggere l'attribuzione AGPL nella nostra licenza.

## Compilare senza toolchain Rust in locale

```bash
docker/check.sh                                   # feature di default
docker/check.sh --features flutter,linux-pkg-config
```

Usa la feature `linux-pkg-config` per prendere vpx/aom/libyuv/opus da apt invece
che da vcpkg: immagine pronta in ~1 minuto invece di ~1 ora. Il repo è montato in
sola lettura e copiato dentro il container, il working tree non viene toccato.

Solo per verificare che compili. Le build di release devono passare da vcpkg come
la CI.

## Fail-closed sui server

Il client **rifiuta di avviarsi** se al build mancano `RENDEZVOUS_SERVER` o
`RS_PUB_KEY`, o se la chiave configurata è quella pubblica di RustDesk. Senza
questo controllo ricadrebbe su `rs-ny.rustdesk.com` e `admin.rustdesk.com` senza
segnalare nulla. Vedi `ensure_own_server_configured()` in
[src/clanto/config.rs](src/clanto/config.rs), chiamata **dopo**
`load_custom_client()` perché
anche quello può fornire server e chiave.

Con server e chiave fissati al build (`servers_fixed_at_build()` in
`src/clanto/config.rs`) niente può sostituirli dopo l'avvio: su Windows
`get_license_from_exe_name()` restituisce errore, quindi rinominare l'eseguibile
non cambia host né chiave, e `read_custom_client()` ignora `custom.txt` con un
warning. Le build di sviluppo senza server restano invariate.

Nessun traffico verso server di terzi: `webrtc_allowed()` e `public_stun_allowed()` in
`src/clanto/config.rs` valgono `false`. WebRTC userebbe i STUN pubblici scritti in
`hbb_common`, e `test_ipv6()` li interrogherebbe a ogni avvio solo per un IPv6 punch che
con i nostri server è spento. Si riaccendono quando ci sarà uno STUN/TURN Clanto.

Per una build di sviluppo locale:

```bash
CLANTO_ALLOW_PUBLIC_SERVER=1 cargo build ...
```

Degrada il blocco a warning. Non distribuire binari compilati così.

### Nessuna via per cambiare i server a runtime

`apply_build_config()` mette in `OVERWRITE_SETTINGS` tutte e quattro le chiavi
dei server — `custom-rendezvous-server`, `key`, `api-server` e `relay-server` —
perché `is_option_can_save()` rifiuta la scrittura di qualsiasi chiave presente
lì, **anche se il valore è vuoto**, e cancella un valore già salvato. `relay-server`
vuoto significa "quello annunciato dal rendezvous", che è il comportamento
voluto: `RELAY_SERVER` esiste solo per fissarne uno.

Le voci di interfaccia sono chiuse a monte: `hide-server-settings` in
`BUILTIN_SETTINGS` nasconde "ID/Relay Server" su desktop e mobile, e il **lettore
di codici QR è stato rimosso** — era l'ultima strada per importare una
configurazione server, e con esso sono usciti `qr_code_scanner`, `zxing2`,
`image_picker` e le due autorizzazioni iOS di fotocamera e libreria foto.

## Cosa controlla la CI prima di compilare

Il job `verify-secrets` in `flutter-build.yml` blocca tutti i build se:

1. mancano i secret dei server
2. la versione in `Cargo.toml` non coincide con i 7 file che non la derivano da lui
   (`pubspec.yaml`, `env.VERSION` ×2, i due rpm-flutter, i due AppImage)
3. `libs/hbb_common` non è allineato al submodule che il tag upstream pinna —
   un disallineamento fa fallire *ogni* job di build con `E0432`
4. resta qualche nome di artefatto `rustdesk-*`

`build.py` riscrive da sé la versione in `PKGBUILD`, `rpm.spec` e `rpm-suse.spec`
leggendola da `Cargo.toml`: quei file non vanno aggiornati a mano.

## Firma Apple locale

Il team Apple non è salvato nei progetti iOS o macOS: `DEVELOPMENT_TEAM` legge
`APPLE_DEVELOPMENT_TEAM` dalla configurazione opzionale e ignorata da Git
`flutter/ios/Flutter/Local.xcconfig` oppure
`flutter/macos/Runner/Configs/Local.xcconfig`. Per compilare e firmare localmente
con il proprio account, creare il file della piattaforma interessata con il
proprio team:

```xcconfig
APPLE_DEVELOPMENT_TEAM = IL_TUO_TEAM_ID
```

Per iOS, per lasciare a Xcode la scelta del profilo locale, aggiungere anche:

```xcconfig
CODE_SIGN_STYLE = Automatic
PROVISIONING_PROFILE_SPECIFIER =
```

Xcode può quindi scegliere un profilo iOS del proprio account senza modificare
file versionati. In CI `APPLE_TEAM_ID` è la sola fonte del team: viene scritto
nelle configurazioni temporanee di entrambe le piattaforme e nella copia
temporanea di `exportOptions.plist`. Per iOS, il `TeamIdentifier` estratto da
`PROVISIONING_PROFILE_BASE64` è un controllo incrociato obbligatorio. La firma
macOS finale resta l'attuale `codesign` esterno, ma ne viene verificato il team.
I job stampano il team configurato e verificano quello della firma prima di
pubblicare l'artefatto.

## Aggiungere funzionalità senza pagarle a ogni merge

Un hook, non venti modifiche sparse: la logica in un modulo proprio, e **una riga**
di chiamata nel file upstream. Una riga aggiunta si auto-mergia quasi sempre;
trenta righe intrecciate nel corpo di una funzione upstream conflittano sempre.

Dove possibile usare i meccanismi che esistono già invece di modificare il codice:
`DEFAULT_SETTINGS` / `OVERWRITE_SETTINGS` per le opzioni, `get_app_name()` invece
dei letterali, `option_env!` per i valori iniettati al build.

I moduli interamente Clanto si trovano in `src/clanto/`, `flutter/lib/clanto/`,
nel package Android `it.clanto.clantodesk.clanto` e, quando Xcode lo richiede,
in `flutter/ios/Runner/`. Non si spostano invece in una cartella radice unica i
file di packaging, le risorse o gli hook che devono restare nei percorsi attesi
dai rispettivi tool. L'inventario di provenienza e licenza è in
[CLANTO-CONTRIBUTIONS.md](CLANTO-CONTRIBUTIONS.md).

## Predefiniti per i dispositivi nuovi

`apply_new_peer_defaults()` in `src/clanto/config.rs` mette in `DEFAULT_DISPLAY_SETTINGS`
(non in OVERWRITE: la scelta dell'utente deve poter vincere) scala adattiva,
appunti e audio disattivati, copia/incolla di file non consentito, cursore remoto
visibile. Valgono per i peer senza file in `peers/` e per i campi mancanti di uno
esistente; un `.toml` già completo non cambia. Precedenza in `UserDefaultConfig`:
valore salvato dall'utente in `_default` > predefinito Clanto > predefinito upstream.

**Trappola:** `is_custom_client()` vale `false` per ClantoDesk, quindi upstream userebbe
`''` come «spento»: scrivere `''` in `_default` *cancella* la scelta e farebbe tornare il
predefinito `Y`. `defaultOptionYes`/`defaultOptionNo` in `flutter/lib/common.dart` valgono
perciò sempre `'Y'`/`'N'`. Chi aggiunge un predefinito nuovo non deve reintrodurre `''`. Chi
aveva già tolto la spunta (valore `''`) vede il nuovo predefinito.

## Popup modalità privacy all'ingresso

`flutter/lib/clanto/privacy_prompt.dart`, agganciato da una riga in `FFI.onEvent2UIRgba`
(`model.dart`, primo frame: a quel punto peer info, permessi e `platform_additions` sono
arrivati). Solo sessioni di controllo remoto, non in sola visualizzazione, non per il ruolo
solo-entrante, una volta per `PeerInfo` (le riconnessioni automatiche non lo ripropongono).
«Attiva» riusa `toolbarPrivacyMode()`: stessa lista e stesso click della toolbar (prima
implementazione se ce n'è più di una). Se la lista è vuota il popup è solo informativo.

- Per dispositivo: casella «Non chiedere più», opzione flutter del peer `clanto-skip-privacy-prompt=Y`.
- Globale: opzione di `Config` `clanto-privacy-prompt`, `N` = mai il popup (assente o `Y` = attivo).
  Si cambia dalle impostazioni (desktop e mobile, «Other Default Options») o da riga di comando
  (installato, da amministratore): `clantodesk --option clanto-privacy-prompt N`. Su file:
  `ClantoDesk2.toml`, sezione `[options]`, `clanto-privacy-prompt = "N"`. È `Config` e non
  `LocalConfig` perché `--option` scrive solo `Config`, e la GUI la legge con `mainGetOptionSync`.
- I testi italiani sono compilati in `it.rs` (in questo fork l'italiano è la lingua principale).

## Predefiniti del PC controllato

`apply_host_defaults()` in `src/clanto/config.rs` mette in `DEFAULT_SETTINGS` (modificabili
dalle impostazioni, un valore già salvato vince): disconnessione per inattività attiva con
timeout 10 **minuti** (`get_auto_disconenct_timer`, `connection.rs`), scoperta LAN, tunnel TCP,
riavvio remoto e terminale spenti. Per le chiavi `enable-*` la UI scrive `'Y'` per «acceso» solo
perché `defaultOptionYes` è esplicito (vedi sopra): con `''` non si potrebbero riaccendere.
L'opzione host `clanto-temp-password-rotate-minutes` ruota la password temporanea dopo 1-1440
minuti; assente o `0` lascia invariato il comportamento senza rotazione.

## Report sessioni

L'host conserva uno storico locale delle sessioni in entrata nella sottocartella
`session-reports` della cartella di configurazione ClantoDesk, in file JSON Lines mensili.
Ogni record riporta metadati della sessione: orari e durata, ID e nome del peer, indirizzo IP,
tipo e metodo di accesso, esito della postura, permessi effettivamente usati e metadati dei
trasferimenti file. Sono registrati separatamente anche i tentativi rifiutati. Non vengono mai
salvati il testo degli appunti, l'input da tastiera o altri contenuti della sessione.

`clanto-session-log-days` stabilisce la conservazione, predefinita a 180 giorni; `0` disabilita
il registro ed elimina i file esistenti. Il report è consultabile ed esportabile dalle impostazioni
di ClantoDesk e rispetta il relativo blocco amministrativo. I record contengono metadati personali
dei peer: limitare la conservazione allo stretto necessario e proteggere l'accesso alle impostazioni.

## Criteri di gruppo (ADMX)

I template sono in `res/admx/clantodesk.admx`, con le risorse `en-US` e `it-IT`.
Copiarli nella cartella locale `PolicyDefinitions` oppure nel Central Store del dominio;
in Intune si importa lo stesso ADMX. I criteri vengono letti da
`HKLM\Software\Policies\Clanto\ClantoDesk` una sola volta all'avvio: dopo `gpupdate`,
riavviare il servizio ClantoDesk. Il profilo `restrittivo` dello script RMM scrive le stesse
chiavi in `HKLM\SOFTWARE\Policies\Clanto\ClantoDesk` e riavvia il servizio solo se cambiano,
quindi blocca le impostazioni anche fuori dominio; su una chiave gestita dal dominio prevale il
GPO, perciò usare un solo canale. L'elenco `id-whitelist` è gestibile sia da GPO sia dal profilo
RMM. Le chiavi dei server
(`custom-rendezvous-server`, `key`, `api-server`, `relay-server`) non sono mai gestibili via GPO.

## Accesso solo da dispositivi protetti

Opzione host `clanto-require-protected-device` (default `N`), nella sezione «Sicurezza in ingresso»
delle impostazioni. Con `N` il login è identico a upstream: l'host non chiede nulla e non fa
chiamate di rete. Con `Y`, dopo l'allowlist per ID e prima di qualunque verifica della password,
l'host manda la postura
dichiarata dal controller a `https://clanto.it/api/clantodesk-posture.php` e rifiuta se la risposta
non è `ok`. **Fail-closed**: API irraggiungibile, timeout di 5 s, JSON non valido, postura o firma
mancanti valgono rifiuto. Il PC host deve quindi raggiungere `clanto.it`.

- **Controller.** `src/clanto/posture.rs`: `collect()` raccoglie le chiavi di `POSTURE_KEYS` (sistema,
  patch, antivirus, firewall, firmware, rete...), `unknown` dove non arriva senza privilegi o entro il
  tempo (8 s su Windows, 3 s altrove). Il risultato resta in cache per 10 minuti; `prewarm()` parte in
  `Client::start`, così il login non aspetta le sonde. Mai SSID, hostname, utenti, IP o MAC.
- **Firma.** HMAC-SHA256 sulla postura, legato a ID dell'host, `challenge` del messaggio `Hash` e
  versione. Il formato canonico sta in `canonical_message()` e **deve coincidere byte per byte** con la
  PHP: cambiarlo da una sola parte rifiuta tutti i login. La chiave è per release: `build.rs` la deriva
  da `CLANTO_POSTURE_MASTER` (secret GitHub, lo stesso valore va nell'env del PHP) e la incorpora
  mascherata; il master non entra mai nel binario. Senza secret la build riesce, ma i suoi controller
  vengono rifiutati dagli host con l'opzione attiva. È una misura contro i dispositivi trascurati, non
  contro chi ricompila il client.
- **Campi proto.** `clanto_posture = 1000` e `clanto_posture_mac = 1001` in `LoginRequest`: numeri
  alti per non collidere con i campi che upstream aggiungerà. Al porting vanno conservati.
- **Bypass.** La PHP restituisce le regole fallite con `bypassable`. L'opzione host
  `clanto-posture-bypass` (id separati da virgola) le ignora su quel PC; `decide()` non lascia mai
  passare un id `core:` (firma, coerenza, versione revocata). Ogni accesso entrato con un bypass va
  nell'audit. Lo script RMM, profilo `restrittivo`, svuota i bypass.
- **Policy.** Vive solo sul server, in `posture-policy.json` accanto alla PHP: regole, tabella EOL per
  edizione, gruppi di host, fasce orarie. Si cambia senza rilasciare il client. Una regola malformata
  blocca tutti gli accessi (`core:policy:<id>`): provarla prima di pubblicarla.
- **Limite noto.** Con l'accesso diretto via IP il controller firma con l'IP al posto dell'ID e la
  verifica fallisce sempre: con l'opzione attiva l'accesso via IP è di fatto escluso.

## Due varianti Android: client e host

`CONN_TYPE`, iniettata al build, decide il **ruolo** della build. È letta due
volte dalla stessa variabile — da `option_env!` in `src/clanto/config.rs`, che la
mette in `HARD_SETTINGS["conn-type"]`, e da `System.getenv` in
`flutter/android/app/build.gradle` — proprio perché core e manifest non possano
divergere.

| `CONN_TYPE` | Ruolo | `applicationId` | Nome | Dove va |
|---|---|---|---|---|
| `outgoing` | solo client | `it.clanto.clantodesk` | ClantoDesk | Play Store |
| `incoming` | solo host | `it.clanto.clantodesk.host` | ClantoDesk Host | monitor interattivi, a mano |
| non impostata | completa | `it.clanto.clantodesk` | ClantoDesk | desktop, dove non si applica |

Il lato client sparisce da `initPages()` con `isIncomingOnly()`; il lato
controllato sparisce perché `RendezvousMediator::start_all` non registra il
dispositivo (`src/rendezvous_mediator.rs`), quindi **non ha un ID
raggiungibile**. Non è un'interfaccia nascosta: è l'assenza dalla rete.

Le due varianti escono dallo **stesso** run di `android-only.yml`: la matrice di
`build-rustdesk-android` moltiplica ogni architettura per i due ruoli, e il
prefisso `host-` nel nome file tiene separati gli artefatti sulla release.
`build-rustdesk-android-universal` invece è fissato a `outgoing`, perché l'APK
universale e l'AAB sono la variante del Play Store.

| Artefatto | Ruolo |
|---|---|
| `clantodesk-X.Y.Z-<arch>.apk` | client |
| `clantodesk-X.Y.Z-universal.apk` | client |
| `clantodesk-X.Y.Z-universal.aab` | client, quello che va su Play |
| `clantodesk-X.Y.Z-host-<arch>.apk` | host, nessun AAB |

I link `clantodesk://` li apre solo il client: lo schema del filtro di deep link nel manifest
principale è il segnaposto `${deepLinkScheme}`, che `build.gradle` imposta a `clantodesk-host` per
la variante host. Con entrambe le app installate Android non chiede più quale usare.

**I due `applicationId` devono restare distinti.** Con lo stesso package e la
stessa chiave di firma, Play prende in carico anche le installazioni fatte a
mano: un monitor interattivo verrebbe aggiornato alla variante client e non
potrebbe più essere assistito.

### Invarianti che ogni porting da upstream deve preservare

Il fork non fa `git merge upstream`: le modifiche si riportano a mano. Non
esiste quindi un conflitto che avvisi, e questi punti vanno verificati a mano.

1. Il blocco `CONN_TYPE` resta **in `apply_build_config()`**. Mai una
   sostituzione `sed` nel workflow: il porting guarda il codice, non le
   pipeline, e una sostituzione mancata non fallisce — produce in silenzio la
   variante permissiva.
2. Un valore di `CONN_TYPE` non riconosciuto **fa fallire il build**, sia in
   Rust sia in Gradle. Non deve mai degradare in "build completa".
3. `src/main/AndroidManifest-client.xml` è una copia **ridotta** del manifest
   principale. Ogni modifica al principale va valutata anche lì.
4. `android:label` passa dai segnaposto `${appLabel}` e `${inputLabel}`. Il nome
   visibile **non** si cambia toccando `config::APP_NAME`: quello alimenta i
   percorsi di configurazione.
5. Lo step `Verify build role` di `flutter-build.yml` è l'unico controllo che
   non dipende dalla memoria di nessuno. Se lo si tocca, si tocca la sola rete
   di sicurezza che resta.

## Restrizioni in uscita

Le opzioni `clanto-outgoing-*` regolano ciò che **questo PC**, quando fa da
controller, può fare: trasferimento file e copia/incolla file, appunti, port
forwarding/RDP, terminale, stampa remota e registrazione locale. Il controllo è
applicato prima della creazione della sessione e nei percorsi di clipboard,
stampa e registrazione; l'interfaccia mostra inoltre le impostazioni bloccate
quando arrivano da Criteri di gruppo.

La restrizione vale solo per ClantoDesk in esecuzione su questo PC. Non
impedisce allo stesso utente di usare un altro programma per svolgere la stessa
operazione.

## Password locale delle impostazioni

`flutter/lib/clanto/settings_password.dart`. Nasce per i monitor interattivi:
`local_auth` su un dispositivo senza PIN né biometria non ha nulla da
verificare, e `ClantoSettingsAuth.authenticate` apriva le impostazioni a
chiunque.

Se impostata, la password **ha la precedenza** sull'autenticazione di sistema:
su un apparato condiviso il PIN del dispositivo è spesso noto a tutti quelli che
lo usano, quindi non è un segreto utilizzabile.

Con l'ambito `settings+host` protegge anche la scheda della schermata host, che
mostra ID e password di connessione. Su un monitor d'aula quelle credenziali
sono fisse: senza il blocco chiunque le fotografa e rientra quando vuole.

Memorizzata come `pbkdf2-sha256$<iter>$<salt>$<dk>` nelle opzioni locali, mai in
chiaro. `crypto` è dichiarato in `pubspec.yaml` proprio per questo: è Dart puro,
non aggiunge codice nativo.

- Rimuoverla richiede di conoscerla, altrimenti chi ha il dispositivo in mano la
  disattiva e apre le impostazioni.
- **Dimenticarla non ha recupero in-app**: si svuotano i dati dell'app o si
  reinstalla. Su un monitor già consegnato va messa in verbale.
- Il blocco della schermata host è agganciato al tocco della scheda in
  `_onNavigationTap`. Se un percorso futuro portasse a `ServerPage` senza passare
  da lì, salterebbe il controllo.

## Clipboard su Wayland senza data-control

Su Wayland senza `ext/zwlr_data_control` (ChromeOS Crostini, sommelier) clipboard-master e arboard
ripiegano su X11/Xwayland, che non vede le selezioni delle app Wayland: il testo non passa in nessuna
direzione. In quel caso, e solo nel processo Flutter del controller, `src/platform/linux_gtk_clipboard.rs`
usa `GtkClipboard` sulla connessione Wayland della finestra (solo testo; file e immagini restano come
sono). Si attiva se `WAYLAND_DISPLAY` è impostata, il display GDK è Wayland e la stessa sonda di
clipboard-master (`is_primary_selection_supported`) fallisce; con `GDK_BACKEND=x11`, X11, KDE e wlroots
il percorso resta quello vecchio. Gli agganci sono in `clipboard.rs` (lettura, scrittura, listener) e
`client.rs` (`init()`): tutte le chiamate GTK passano da `glib::idle_add_once` sul thread principale.

## Debito noto

- `res/msi/Package/License.rtf` è un testo **provvisorio** che rimanda alla policy
  pubblicata: va sostituito con l'EULA vero, e serve la versione inglese se si
  distribuisce fuori dall'Italia.
- `.github/workflows/playground.yml` è un workflow sperimentale di upstream, con
  nomi `rustdesk-*` e riferimenti a `RustDesk.app` che non esiste più: se lanciato
  a mano pubblicherebbe artefatti con il nome sbagliato sulla release `nightly`.
