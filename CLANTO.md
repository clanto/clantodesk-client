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

Le chiavi nuove del fork vanno **solo** in `template.rs`, `it.rs` e, se il testo
inglese differisce dalla chiave, `en.rs`. Non negli altri file, a differenza di
quanto chiede `AGENTS.md`: `translate()` tratta una chiave assente come una con
valore `""` (ricade su `en.rs`), e ogni file toccato in più conflitta a ogni
aggiornamento upstream delle traduzioni.

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
docker/test-all.sh                                # tutto, con riepilogo finale
docker/check.sh                                   # cargo check (linux-pkg-config)
docker/check.sh --test clanto:: core_main::       # test della lib filtrati
docker/flutter-check.sh                           # bridge, flutter analyze/test, check con flutter
docker/e2e/run.sh                                 # end-to-end: 2FA, controller fidati, fasce orarie, ticket, forwarding, webcam
docker/security-scan.sh                           # Semgrep (blocca) e OSV-Scanner (informa)
docker/android-build.sh [abi] [outgoing|incoming] # APK come la CI (firma di debug) in target/android/
```

`--features flutter` richiede `src/bridge_generated.rs`, che la CI genera: per la parte Flutter
usare `docker/flutter-check.sh`, che lo genera come `bridge.yml`. Il workflow `security-scan.yml`
esegue gli stessi strumenti di `docker/security-scan.sh`; le esclusioni Semgrep sono motivate nello
script e i falsi positivi nei test sono marcati `nosemgrep`. `docker/e2e/run.sh` avvia hbbs/hbbr
ufficiali, un host reale (`--server`) e un controller headless con due utenti; l'harness
`docker/e2e/e2e_harness.rs` entra solo nella copia del repo nel volume di lavoro. `docker/android-build.sh`
usa SDK, NDK, cargo-ndk e Flutter delle versioni del workflow; vcpkg (commit `VCPKG_COMMIT_ID`), le sue
librerie e la cache di Gradle stanno in volumi, quindi solo la prima build è lunga.

**Prestazioni e I/O** (`docker/common.sh`): il repo è montato in sola lettura e allineato con
`rsync` incrementale in un volume per profilo (`clantodesk-work-{check,flutter,e2e}`); `target/`,
registry e git di Cargo e pub cache stanno in volumi nominati sul disco della VM di Docker, non sul
filesystem dell'host; `/tmp` è un tmpfs. I Dockerfile usano le cache BuildKit per apt e Cargo.
Dopo il primo giro si ricompila solo ciò che è cambiato. Gli script convertono da soli i percorsi
su Windows (Git Bash). Per ripartire da zero: `docker volume rm` dei volumi `clantodesk-*`.

Usa la feature `linux-pkg-config` per prendere vpx/aom/libyuv/opus da apt invece
che da vcpkg: immagine pronta in ~1 minuto invece di ~1 ora. Il working tree non
viene mai toccato.

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
configurazione server, e con esso sono usciti `qr_code_scanner`, `zxing2` e
`image_picker`. Su iOS la fotocamera è tornata solo per il forwarding webcam
(`NSCameraUsageDescription`, aperta dal menu Forwarding), non per i codici QR.

## Cosa controlla la CI prima di compilare

Il job `verify-secrets` in `flutter-build.yml` blocca tutti i build se:

1. mancano i secret dei server
2. la versione in `Cargo.toml` non coincide con i 6 file che non la derivano da lui
   (`pubspec.yaml`, `env.VERSION` di `flutter-build.yml`, i due rpm-flutter, i due AppImage)
3. il commit di `libs/hbb_common` pinnato dal tag upstream non è un antenato del
   commit del fork Clanto — il fork può essere avanti, ma non può perdere modifiche upstream
4. resta qualche nome di artefatto `rustdesk-*`

Dopo la firma, il passo «Verify signatures and Clanto EKU» di `flutter-build.yml` ferma il build
Windows se un `.exe`/`.dll` non porta l'EKU Clanto (vedi [Firma del codice](#firma-del-codice)).

`build.py` riscrive da sé la versione in `PKGBUILD`, `rpm.spec` e `rpm-suse.spec`
leggendola da `Cargo.toml`: quei file non vanno aggiornati a mano.

## Credenziali protette

I segreti locali usano due chiavi casuali indipendenti. La chiave **utente**
protegge le credenziali necessarie per collegarsi ad altri PC: password e opzioni
del peer (`os-username`, `os-password`, `rdp_username`, `rdp_password`), token e
profilo dell'account locale, rubrica e gruppi. La chiave **macchina** protegge la
configurazione globale dell'host: ID, password permanente, chiave privata, PIN di
sblocco, password Socks5, dispositivi attendibili, segreto TOTP e token Telegram.
L'URI `otpauth` usa il nome applicazione configurato a runtime (`ClantoDesk`) come issuer e label;
questi metadati non sono salvati nel record TOTP e non partecipano al calcolo o alla verifica del
codice, quindi le configurazioni 2FA esistenti restano valide.

La posizione dipende dalla piattaforma; `Config::path(...)` indica sempre la
cartella di configurazione del profilo corrente.

| Piattaforma | Chiave utente | Chiave macchina | Ripiego |
| --- | --- | --- | --- |
| Windows | `Config::path("user.key")`, blob DPAPI utente | `Config::path("machine.key")`, blob DPAPI macchina | Nessuno; SYSTEM non apre mai la chiave utente |
| macOS | Keychain, generic password servizio `it.clanto.ClantoDesk`, account `user-key`, `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`, non sincronizzabile | `~/Library/Preferences/it.clanto.ClantoDesk/machine.key`, file `0600` | Il precedente `~/Library/Preferences/it.clanto.ClantoDesk/user.key` resta utilizzabile finché l'importazione non è verificata; non vengono creati nuovi file utente |
| iOS | Keychain dell'app, stessa voce e accessibilità di macOS, non sincronizzabile | `<contenitore app>/Documents/machine.key`, file `0600` | Il precedente `<contenitore app>/Documents/user.key` resta utilizzabile finché l'importazione non è verificata |
| Linux | Secret Service sul bus di sessione, collection alias `default`, item con attributi `application=it.clanto.ClantoDesk` e `account=user-key`; l'uso viene registrato in `user.key.backend` nella cartella di configurazione, file `0600` con contenuto `secret-service` | `$XDG_CONFIG_HOME/clantodesk/machine.key`, oppure `~/.config/clantodesk/machine.key`, file `0600` | Al primo avvio, senza marcatore, se bus o Secret Service non esistono usa `user.key` nella stessa cartella, file `0600`; con il marcatore non crea una nuova chiave se Secret Service non è raggiungibile, e una collection esistente ma bloccata non abilita il ripiego |
| Android | alias non esportabile `it.clanto.ClantoDesk.user-key` in `AndroidKeyStore`; i 32 byte sono AES-GCM in `${Context.filesDir}/clantodesk-user-key.v1` | `Config::path("machine.key")` nei dati privati dell'app, file `0600` | `Config::path("user.key")` nei dati privati dell'app se Android Keystore non è disponibile |

I processi root/LaunchDaemon su macOS e Linux e il servizio SYSTEM su Windows non
richiedono mai la chiave utente. Su Linux la chiave viene risolta prima della
registrazione del provider, fuori dal runtime Tokio, e resta in cache per il
processo. Se un accesso da un worker Tokio trova la cache vuota, restituisce chiave
non disponibile e avvia la risoluzione su un thread dedicato, senza attendere il
prompt. L'annullamento dello sblocco della collection `default` non crea una nuova
collection né un nuovo `user.key`. I due manifest Android sono stati valutati: il
Keystore non richiede permessi né componenti aggiuntivi, quindi il manifest ridotto
della variante client resta invariato.

Su Linux il marcatore si trova in
`$XDG_CONFIG_HOME/clantodesk/user.key.backend`, oppure
`~/.config/clantodesk/user.key.backend`. Se Secret Service è temporaneamente
indisponibile, il marcatore impedisce di generare un `user.key` diverso: la chiave
resta non disponibile e la risoluzione viene ritentata in background. Per
abbandonare deliberatamente il keyring, chiudere ClantoDesk, eliminare il marcatore
e riavviare senza Secret Service; questa operazione accetta di rendere
irrecuperabili le credenziali cifrate con la precedente chiave del keyring, che
dovranno essere inserite di nuovo.

Se esiste il `user.key` creato dalla prima parte, la chiave viene copiata nel
portachiavi di sistema solo quando non vi è già una voce. Il file viene eliminato
soltanto dopo una rilettura identica dal portachiavi; errore, voce mancante o valore
diverso conservano il file. Una voce e un file esistenti ma diversi causano un
errore e non vengono sovrascritti. Se un backend non disponibile ammette il
ripiego (Linux e Android), il file continua a essere usato; una chiave nuova viene
generata solo quando né il backend disponibile né il file contengono una chiave.
Su Android l'esistenza del blob cifrato viene verificata separatamente: un blob
già presente ma temporaneamente non decifrabile non genera mai un nuovo fallback.

`--import-config` considera autorevole la configurazione del servizio già
installato. Se il relativo `ClantoDesk.toml` contiene un `id`, un `enc_id` o una
`key_pair` non vuoti, non importa dal profilo utente né `Config`, né `Config2`, né
`machine.key`, anche se i file del profilo sono più recenti. L'importazione dal
profilo resta disponibile soltanto alla prima installazione, quando il `Config`
del servizio è assente oppure tutti e tre i campi di identità sono vuoti, e
continuano a valere i controlli sulle date preesistenti. Prima di sostituire un
`ClantoDesk.toml` o `ClantoDesk2.toml` già presente ne salva una copia affiancata
con suffisso `.pre-import`.

Durante l'importazione di prima installazione viene copiato anche `machine.key`.
Se il `Config2` della sorgente è più recente, viene importato insieme alla relativa
chiave. Altrimenti le opzioni più recenti della destinazione restano intatte:
prima della sostituzione i segreti del `Config2` vengono decifrati con la chiave
precedente e poi riscritti con quella nuova. Un campo che non può essere
decifrato viene segnalato nel log e il relativo valore cifrato resta invariato
nello storage. Se venisse importato il solo `Config2` mentre nella sorgente
esiste `machine.key`, `--import-config` rifiuta l'operazione: importare i segreti
senza il relativo `Config` potrebbe associare configurazione e chiave macchina
diverse.

I valori nuovi hanno prefisso `02` per l'ambito macchina e `03` per l'ambito
utente. I valori storici `00` continuano a essere letti con la chiave derivata
dall'UUID (incluso il fallback alla chiave pubblica) e vengono riscritti nel nuovo
formato al primo salvataggio utile. In assenza del provider di chiavi,
`hbb_common` conserva esattamente il formato e il comportamento upstream.
Se invece il provider è registrato ma la chiave dell'ambito non è disponibile,
non viene mai creato un nuovo valore `00`: uno scoped ciphertext già presente
resta invariato e un valore nuovo o modificato non viene salvato. L'errore viene
registrato una sola volta per ambito durante il processo.

Copiare i file TOML, rubrica o gruppi senza il relativo file `.key` su un'altra
macchina, oppure in un altro profilo, rende intenzionalmente indecifrabili i
segreti del relativo ambito. I dati non vengono rigenerati o sovrascritti quando
manca la chiave; in particolare l'ID cifrato viene lasciato intatto. Finché l'ID
non è decifrabile il servizio non si registra né annuncia un ID vuoto al server
rendezvous e segnala periodicamente lo stato nel log. Ogni ciclo riprova, quindi
la registrazione riprende senza riavvio appena la chiave torna disponibile. La
protezione macchina non difende da un processo locale con privilegi
amministrativi.

La password RDP non viene più scritta nelle variabili d'ambiente né passata a
`cmdkey`. Su Windows il port-forward crea direttamente la credenziale generica
`localhost` con `CredWriteW` e persistenza `CRED_PERSIST_SESSION`; una guardia la
elimina con `CredDeleteW` alla chiusura del port-forward, incluse le uscite per
errore. `mstsc` continua a ricevere soltanto `/v:localhost:<porta>`.

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

`apply_host_defaults()` in `src/clanto/config.rs` mette in `HOST_DEFAULTS` → `DEFAULT_SETTINGS`
(modificabili dalle impostazioni, un valore già salvato vince; mai in `OVERWRITE_SETTINGS`). Sono
dieci:

| Chiave | Default |
| --- | --- |
| `allow-auto-disconnect` | `Y` |
| `auto-disconnect-timeout` | `10` (**minuti**, `get_auto_disconenct_timer` in `connection.rs`) |
| `enable-lan-discovery` | `N` |
| `enable-tunnel` | `N` |
| `enable-remote-restart` | `N` |
| `enable-terminal` | `N` |
| `clanto-require-protected-device` | `N` |
| `clanto-posture-bypass` | vuoto |
| `clanto-session-log-days` | `180` |
| `clanto-host-watermark` | `N` |

Per le chiavi `enable-*` la UI scrive `'Y'` per «acceso» solo
perché `defaultOptionYes` è esplicito (vedi sopra): con `''` non si potrebbero riaccendere.
L'opzione host `clanto-temp-password-rotate-minutes` ruota la password temporanea dopo 1-1440
minuti; assente o `0` lascia invariato il comportamento senza rotazione. Si configura nella card
Password delle impostazioni desktop e nella sezione Sicurezza dell'host Android quando il metodo
di verifica usa la password temporanea.

## Permessi per ID autorizzato

`id-whitelist` continua a stabilire quali ID possono entrare e accetta i caratteri jolly `*` e
`?`. L'opzione host `clanto-id-permissions` associa a ciascuna voce dell'elenco un oggetto JSON con
un'etichetta facoltativa `name`, un `profile` (`view`, `support`, `full` o `custom`) e, soltanto per
`custom`, le caselle abilitate in `perms`. Una voce senza configurazione mantiene `full`, per
compatibilità con le installazioni esistenti; un JSON non valido applica invece `view` a tutti gli
ID dell'elenco. Se l'elenco è vuoto, nessun profilo viene applicato.

`view` consente soltanto la visione; `support` abilita tastiera, appunti, file, audio, riavvio,
blocco input, modalità privacy e stampante remota; `full` abilita tutti i dodici permessi. In
`custom` ogni permesso assente vale spento. In caso di più corrispondenze prevale l'ID esatto,
anche nella forma `id@server`, poi il pattern con più caratteri non jolly e infine `*`. Il profilo
viene sempre combinato in AND con i permessi globali dell'host e non può ampliarli; durante una
sessione l'operatore presente può ancora modificarne i permessi dalla finestra di connessione.

## Controllo dei file ricevuti dall'host

Il controllo si applica sul lato host ai file ricevuti dal controller, compresi quelli contenuti
nelle cartelle trasferite. Il controller che riceve file non esegue questa scansione. L'opzione
`clanto-file-scan` accetta `off` (predefinito), `warn` e `block`: con `off` resta invariato il
percorso di trasferimento esistente. Negli altri modi ClantoDesk completa il file con suffisso
`.download`, calcola SHA-256 e lo sottopone al motore prima di rinominarlo al nome finale.

- **Windows:** usa AMSI con nome applicazione `ClantoDesk`, leggendo il file a blocchi nella stessa
  sessione AMSI. Funziona con qualunque prodotto antimalware registrato come provider AMSI, non solo
  Microsoft Defender. Alla scadenza del timeout resta valida la decisione `warn`/`block`, ma Windows
  non permette di interrompere forzatamente una chiamata del provider già in corso. Due limiti
  indipendenti ammettono al massimo un worker di preparazione (sync, metadati e SHA-256) e un worker
  del motore (AMSI o `clamd`). Un provider bloccato trattiene solo il permit del motore: i file
  successivi calcolano comunque dimensione e SHA-256, poi scadono in attesa del motore.
- **macOS e Linux:** usa il protocollo `INSTREAM` del demone ClamAV `clamd`. L'opzione
  `clanto-file-scan-clamd` contiene il percorso di un socket Unix oppure un endpoint TCP
  `host:porta`; il valore predefinito è vuoto.
- **Android, iOS o motore non disponibile:** il file è non scansionabile. Lo stesso vale se AMSI non
  può essere inizializzato o non ha un provider, `clamd` non è configurato o raggiungibile, la
  scansione scade, oppure il file supera `clanto-file-scan-max-mb` (predefinito: 512 MiB).

Un esito pulito consente la rinomina normale. In modalità `warn`, malware e file non scansionabili
sono consegnati ma generano un evento di audit. In modalità `block` entrambi sono rifiutati
**fail-closed**: il `.download` non assume mai il nome finale e viene spostato nella sottocartella
`quarantine` della cartella di configurazione ClantoDesk, con permessi ristretti e nome composto da
timestamp e SHA-256. Il controller riceve un errore leggibile. Log, audit e report sessioni possono
contenere solo nome, dimensione, SHA-256, motore ed esito, mai il contenuto del file.

`clanto-file-direction` restringe separatamente i trasferimenti visti dall'host: `both`
(predefinito), `to-host` (solo controller → host), `from-host` (solo host → controller) oppure
`none`. Il controllo viene eseguito prima di aprire il job e non può riabilitare un trasferimento
negato da `enable-file-transfer`. `clanto-file-blocked-ext` è un elenco di estensioni separate da
virgole, vuoto per impostazione predefinita. I file in ingresso con un'estensione elencata vengono
rifiutati prima della creazione; il confronto non distingue maiuscole e minuscole, usa l'ultima
estensione e rimuove prima punti e spazi finali, quindi `report.pdf.exe`, `A.EXE` e `a.exe.` vengono
tutti riconosciuti quando è bloccata `exe`.

La scansione non garantisce di riconoscere contenuti che il motore non può ispezionare, per esempio
gli archivi protetti da password. Valgono inoltre i limiti configurati nel provider AMSI o in
`clamd`; per una policy che non deve consegnare file non verificati usare `block`. I cinque valori
sono gestibili anche tramite GPO/ADMX e vengono letti una volta all'avvio del servizio.

## Report sessioni

L'host conserva uno storico locale delle sessioni in entrata nella sottocartella
`session-reports` della cartella di configurazione ClantoDesk, in file JSON Lines mensili.
Ogni record riporta metadati della sessione: orari e durata, ID e nome del peer, indirizzo IP,
tipo e metodo di accesso, esito della postura, permessi effettivamente usati e metadati dei
trasferimenti file. Sono registrati separatamente anche i tentativi rifiutati. Non vengono mai
salvati il testo degli appunti, l'input da tastiera o altri contenuti della sessione.
I quattro esiti di sicurezza dei file sono record autonomi con `record_type=file_security` e i soli
campi `occurred_at`, `conn_id`, `event`, `name`, `size`, `sha256`, `engine` e `result`; non includono
contenuto o percorso del file.

Ogni riga è cifrata con la chiave **macchina** (`seal_line()` in `src/clanto/session_log.rs`); la
lettura avviene nel servizio via IPC, come prima. Senza chiave macchina il record non viene scritto
in chiaro ma scartato, con un messaggio nel log: l'audit SIEM resta l'evidenza di riserva. Le righe
in chiaro scritte dalle versioni precedenti restano leggibili e scadono con la conservazione.

`clanto-session-log-days` stabilisce la conservazione, predefinita a 180 giorni; `0` disabilita
il registro ed elimina i file esistenti. Il report è consultabile ed esportabile dalle impostazioni
di ClantoDesk; nella stessa sezione si imposta la conservazione da 0 a 3650 giorni, con conferma
esplicita prima di salvare `0`, e il controllo rispetta il relativo blocco amministrativo. I record contengono metadati personali
dei peer: limitare la conservazione allo stretto necessario e proteggere l'accesso alle impostazioni.

**Integrità.** Ogni riga porta `_seq` (progressivo), `_prev` (impronta della riga precedente) e
`_hash` (SHA-256 di `_prev` e del record senza `_hash`). `report_json()` rilegge i file in ordine e
verifica la catena: ogni record arriva alla UI con `integrity` = `ok` o `broken` (assente per le
righe scritte prima della catena), e il report mostra un pallino verde, rosso o grigio. Una riga
modificata, tolta, spostata o illeggibile rompe la catena da quel punto; il primo record rimasto
dopo la pulizia per conservazione fa da ancora. Chi ha la chiave macchina (amministratore) può
riscrivere la catena intera: contro questo serve la copia esterna dell'audit (SIEM).

**Riferimento ticket.** Facoltativo: `clantodesk://connect/<ID>?ticket=INC-1234` o
`--connect <ID> --ticket INC-1234` (`src/clanto/ticket.rs`). Il controller lo invia nel login
(`clanto_ticket = 1004` in `LoginRequest`), l'host lo aggiunge agli eventi di audit (quindi a
Event Log e SIEM) e al report, accanto al codice sessione; è cercabile ed esportato nel CSV. Testo
ripulito, massimo 64 caratteri; non influisce sull'accesso. Per i software di ticketing
`clantodesk --clanto-session-report [giorni]` stampa il report in JSON (default 7 giorni).

## Integrazione SIEM dell'audit host

L'host può duplicare gli eventi di audit nel registro eventi Application di Windows e verso un
collettore syslog. Entrambe le destinazioni sono disabilitate per impostazione predefinita e si
configurano con queste opzioni:

- `clanto-eventlog-enabled`: `Y` abilita il Windows Event Log.
- `clanto-syslog-enabled`: `Y` abilita l'invio al collettore.
- `clanto-syslog-host`: nome DNS o indirizzo del collettore; è anche il nome server verificato da
  TLS.
- `clanto-syslog-port`: porta del collettore. Se vuota, usa `514` per UDP/TCP e `6514` per TLS.
- `clanto-syslog-protocol`: `tls` (predefinito), `udp` oppure `tcp`. UDP e TCP inviano in chiaro IP, ID e nomi dei
  peer: all'avvio dell'invio il log lo segnala una volta.
- `clanto-syslog-facility`: facility RFC 5424 da 0 a 23; il valore predefinito è 16 (`local0`).
- `clanto-syslog-tls-ca`: percorso del file PEM contenente la CA del collettore. Il trust store TLS
  contiene esclusivamente i certificati validi letti da questo file: non usa le radici di sistema.
  Con percorso assente, file illeggibile o nessun certificato valido, l'invio TLS è fail-closed e
  non apre la connessione.
- `clanto-syslog-format`: `rfc5424` (predefinito) oppure `cef`.

TCP e TLS usano l'octet-counting (`<lunghezza> <messaggio>`) previsto da RFC 6587/RFC 5425. Un
singolo worker asincrono serve una coda limitata a 1000 record: se si riempie elimina il record più
vecchio e aggiunge il totale perso al primo record consegnato successivamente (`lost_records` in
CEF e nel payload JSON, `lostRecords` negli structured data RFC 5424). Dopo un errore TCP/TLS il
record in testa resta in coda e la connessione viene ritentata con backoff esponenziale da 1 a 60
secondi; UDP non ritenta. Gli errori di consegna sono riportati nel log con limitazione di frequenza.

Gli ID seguenti sono il contratto stabile con il SIEM. Lo stesso numero viene emesso come MSGID e
`eventId` negli structured data RFC 5424, Signature ID CEF ed Event ID del Windows Event Log. Non
riassegnare mai un ID, neppure dopo la rimozione di un evento.

| ID | Evento | Severità syslog | Severità CEF |
|---:|---|---:|---:|
| 61100 | Connection opened | 6 (informational) | 3 |
| 61101 | Session opened | 6 (informational) | 3 |
| 61102 | Connection closed | 6 (informational) | 3 |
| 61103 | Session completed | 6 (informational) | 3 |
| 61104 | Controller identity received | 6 (informational) | 3 |
| 61110 | `FileAuditType::RemoteSend` — File sent to peer | 5 (notice) | 5 |
| 61111 | `FileAuditType::RemoteReceive` — File received from peer | 5 (notice) | 5 |
| 61120 | `AlarmAuditType::IpWhitelist` — IP whitelist rejected | 4 (warning) | 7 |
| 61121 | `AlarmAuditType::ExceedThirtyAttempts` — Login attempt limit exceeded | 4 (warning) | 7 |
| 61122 | `AlarmAuditType::SixAttemptsWithinOneMinute` — Login rate limit exceeded | 4 (warning) | 7 |
| 61123 | `AlarmAuditType::ExceedIPv6PrefixAttempts` — IPv6 prefix attempt limit exceeded | 4 (warning) | 7 |
| 61124 | `AlarmAuditType::TerminalOsLoginBackoff` — Terminal OS login backoff | 4 (warning) | 7 |
| 61125 | `AlarmAuditType::TerminalOsLoginConcurrency` — Terminal OS login concurrency rejected | 4 (warning) | 7 |
| 61126 | `AlarmAuditType::SessionScopeViolation` — Session scope violation | 4 (warning) | 7 |
| 61127 | `AlarmAuditType::IdWhitelist` — ID whitelist rejected | 4 (warning) | 7 |
| 61128 | `AlarmAuditType::PostureRejected` — Device posture rejected | 4 (warning) | 7 |
| 61129 | `AlarmAuditType::PasswordWrong` — Password authentication failed | 4 (warning) | 7 |
| 61130 | `AlarmAuditType::TwoFactorWrong` — Two-factor authentication failed | 4 (warning) | 7 |
| 61140 | Postura superata (`clanto_posture`) — Device posture passed | 6 (informational) | 3 |
| 61141 | Postura con bypass (`clanto_posture_bypass`) — Device posture bypassed | 5 (notice) | 5 |
| 61150 | Temporary password rotated | 5 (notice) | 5 |
| 61160 | Malware detected in incoming file | 4 (warning) | 7 |
| 61161 | Incoming file could not be scanned | 4 (warning) | 7 |
| 61162 | File transfer blocked by direction policy | 4 (warning) | 7 |
| 61163 | Incoming file blocked by extension | 4 (warning) | 7 |
| 61164 | Incoming file scanned clean | 6 (informational) | 3 |
| 61180 | ClantoDesk binary tampered (not signed by Clanto) | 2 (critical) | 8 |
| 61170 | Forwarding channel opened (`clanto_forward_opened`) | 5 (notice) | 5 |
| 61171 | Forwarding channel denied (`clanto_forward_denied`) | 4 (warning) | 7 |
| 61172 | Forwarding listener opened (`clanto_forward_listen`) | 5 (notice) | 5 |
| 61173 | Forwarding listener denied (`clanto_forward_listen_denied`) | 4 (warning) | 7 |
| 61174 | Controller camera opened (`clanto_forward_camera_opened`) | 5 (notice) | 5 |
| 61175 | Controller camera denied (`clanto_forward_camera_denied`) | 4 (warning) | 7 |
| 61176 | Controller camera closed (`clanto_forward_camera_closed`) | 6 (informational) | 3 |
| 61999 | Unknown audit event | 5 (notice) | 5 |

Gli ID legacy 61000, 61001, 61002 e 61099 sono riservati permanentemente e non devono essere
riutilizzati.

## Criteri di gruppo (ADMX)

I template sono in `res/admx/clantodesk.admx`, con le risorse `en-US` e `it-IT`.
Copiarli nella cartella locale `PolicyDefinitions` oppure nel Central Store del dominio;
in Intune si importa lo stesso ADMX. I criteri vengono letti da
`HKLM\Software\Policies\Clanto\ClantoDesk` una sola volta all'avvio: dopo `gpupdate`,
riavviare il servizio ClantoDesk.

**Solo Windows.** `gpo::apply()` in `src/clanto/gpo.rs` è `#[cfg(windows)]`: legge il registro e
copia i valori in `OVERWRITE_SETTINGS`. Sulle altre piattaforme è una funzione vuota, quindi un
criterio non ha effetto e le opzioni si impostano da interfaccia o con
`clantodesk --option <chiave> <valore>`.

L'elenco `id-whitelist`, i relativi profili `clanto-id-permissions` e le cinque opzioni del
controllo file sono gestibili via GPO. Per una policy restrittiva impostare `clanto-file-scan=block`
e bloccare le estensioni eseguibili e di script con `clanto-file-blocked-ext`. Le chiavi dei server
(`custom-rendezvous-server`, `key`, `api-server`, `relay-server`) non sono mai gestibili via GPO.
La policy `LockSettings` imposta `clanto-lock-settings=Y` e rende tutte le impostazioni di sola
lettura, amministratori compresi, lasciando modificabili soltanto tema e lingua. La chiave è
accettata esclusivamente dal caricamento GPO: interfaccia, IPC e riga di comando non possono
impostarla. Il backend rifiuta inoltre le altre scritture richieste dall'utente con il messaggio
`Settings are managed by policy`; le scritture interne del programma restano consentite.
L'elenco completo delle chiavi gestibili è `GPO_KEYS`; la tabella con valori e predefiniti è nel
[README](README.md#opzioni-di-configurazione).

## Configurazione gestita da MDM (Android e iOS)

Sui telefoni le stesse chiavi della GPO (`GPO_KEYS` in `src/clanto/gpo.rs`) si impostano dall'MDM
(Intune, Workspace ONE, Jamf, Android Enterprise...). `src/clanto/managed_config.rs` riceve un JSON
e le mette in `OVERWRITE_SETTINGS` come la GPO: valgono subito, l'interfaccia le mostra fissate e
una chiave tolta dall'MDM torna modificabile. Booleani diventano `Y`/`N`.

- **Android:** managed configurations. Lo schema per la console MDM è
  `flutter/android/app/src/main/res/xml/app_restrictions.xml` (titoli in `values*/clanto_mdm.xml`,
  presi dall'ADMX), dichiarato in entrambi i manifest. `ClantoManagedConfig.kt` lo legge all'avvio
  dell'app e a ogni `ACTION_APPLICATION_RESTRICTIONS_CHANGED`. Le policy Y/N sono di tipo `bool`.
  Un test verifica che ogni chiave della GPO sia nello schema: aggiungendo una policy si rigenera.
- **iOS:** managed app configuration, chiave `com.apple.configuration.managed` (app distribuita
  tramite MDM). `AppDelegate.swift` la legge all'avvio e a ogni modifica e la passa a
  `clanto_apply_managed_config`.

## 2FA senza "Trust this device"

Upstream, dopo un codice 2FA corretto, il controller può chiedere di essere ricordato e l'host salva
hwid, ID, nome e piattaforma **dichiarati dal controller**: per 90 giorni quel controller entra senza
2FA. Sono dati che il controller invia a ogni host a cui si collega, quindi chi gestisce un host li
conosce e un client modificato può replicarli: il secondo fattore si riduce alla password.

Nel fork la richiesta non esiste: `controller_device_trust_allowed()` in `src/clanto/config.rs` vale
`false` (il controller non mostra la casella e non invia l'hwid) e `apply_build_config()` fissa
`enable-trusted-devices=N` in `OVERWRITE_SETTINGS`, così l'host non salva né consulta l'elenco anche
con controller vecchi o modificati. Le voci già salvate restano inerti e scadono da sole. La chiave
non è in `GPO_KEYS` e un test lo verifica.

### Controller fidati, decisi dall'host

L'esenzione la concede solo l'host, con l'opzione `clanto-trusted-controllers` (voci `ID=CHIAVE`
separate da virgole; interfaccia, GPO `TrustedControllers`, oppure `--trust-controller` /
`--untrust-controller` da amministratore su installazione). Logica in
`src/clanto/trusted_controllers.rs`:

- **Controller.** Chiave Ed25519 per utente, generata al primo uso e salvata in `LocalConfig`
  (`clanto-device-key`) cifrata con la chiave **utente** del keystore. Se la chiave utente non è
  disponibile non viene salvata in chiaro: il controller resta senza chiave e l'host chiede il 2FA.
  La pubblica si legge con `--device-key` o in Impostazioni → Sicurezza.
- **Login.** `LoginRequest.clanto_device_key = 1002` e `clanto_device_sig = 1003` (in
  `libs/base/protos/message.proto`, da conservare nei porting). La firma copre
  `clanto-controller-v1`, ID dell'host composto dal controller, `challenge` del messaggio `Hash`,
  ID del controller e chiave pubblica. L'host verifica con **il proprio** `Config::get_id()`: una
  firma ottenuta da un altro host non vale, e con l'accesso via IP l'esenzione non scatta mai.
- **Host.** Hook unico in `handle_login_request_without_validation()`: salta il 2FA solo se la coppia
  ID/chiave è in elenco e la firma è valida; audit `TrustedDevice`. Voci malformate sono ignorate
  (fail-closed). Password, allowlist, postura e filigrana restano invariate.
- **Privacy.** La chiave pubblica viaggia a ogni login, come l'ID del controller che identifica
  già il dispositivo; a differenza dell'`hwid` di upstream non deriva dall'hardware.
- **Limite noto.** L'interfaccia Sciter (build legacy) non ha la gestione dell'elenco: lì si usano
  CLI o GPO.

### 2FA obbligatorio per policy

`clanto-require-2fa=Y` (GPO `RequireTwoFactor`, oppure `--option`) non crea il segreto TOTP, che è
per macchina e va registrato dall'utente: fa rifiutare ogni login finché sull'host il 2FA non è
configurato. Il controllo sta in `src/clanto/two_factor_policy.rs`, con un hook in `connection.rs`
dopo allowlist e permessi per ID e prima della postura; il controller riceve il testo
`clanto_2fa_policy_error` già tradotto dall'host. Con la policy attiva e il 2FA spento, le
impostazioni host mostrano un avviso rosso nella sezione 2FA.

### Fasce orarie di accesso

`clanto-access-hours` (GPO `AccessHours`, sezione «Sicurezza in ingresso») limita le connessioni
in ingresso a fasce orarie, nell'ora locale dell'host: voci `giorni HH:MM-HH:MM[,...]` separate da
`;`, giorni `mon`..`sun` o `lun`..`dom`, intervalli (`mon-fri`) o elenchi (`sat,sun`), `24:00`
come fine giornata, fasce a cavallo della mezzanotte, `none` per nessuna fascia. Vuota = sempre.
Valore non valido = ogni connessione rifiutata (fail-closed). Logica in
`src/clanto/access_hours.rs`, nello stesso hook del login di 2FA e policy
(`crate::clanto::login_block()`), quindi vale per ogni modalità d'accesso, approvazione compresa.
Il controller riceve `clanto_access_hours_error` tradotto dall'host. Chiave protetta dalla
`strategy` del server. Le impostazioni mostrano una fascia per giorno; un valore più ricco
(per esempio da GPO) si modifica come testo.

### Segreto 2FA cifrato dal servizio

Segreto TOTP e token del bot Telegram sono cifrati con `machine.key`, che è per utente di
sistema: su Windows il servizio (SYSTEM) ha una chiave diversa da quella della UI. Se cifrasse la
UI, il servizio non leggerebbe il segreto e accetterebbe la sola password. In
`src/clanto/two_factor_store.rs` la UI desktop manda il valore in chiaro sull'IPC locale
(`clanto-2fa`, `clanto-bot`) e il servizio lo cifra con la propria chiave; `clanto-2fa-state`
riporta lo stato visto dal servizio alle impostazioni. Fail-closed: un segreto presente ma
illeggibile fa rifiutare ogni login con `clanto_2fa_unreadable_error` (stesso hook della policy).
All'avvio la UI reinvia al servizio i valori salvati dalle versioni precedenti, se lei riesce a
leggerli; altrimenti va disattivato e riattivato il 2FA sull'host. `bot` è tra le
`SERVER_PROTECTED_KEYS`: un server non può dirottare i codici su un altro bot.

## Server nella barra di stato

La barra in basso della finestra principale (desktop) dice a quale server si collega il PC
(`main_clanto_server_status`, `src/clanto/config.rs`): se è uno di quelli fissati al build mostra
«Pronto per la connessione tramite server Clanto» con il bollino verde, altrimenti «Pronto, server
non gestito da Clanto: host» con il bollino arancione. Il nome del server è nel tooltip.

## Canale verso `api-server`

Con `API_SERVER` al build il client manda a quel server heartbeat e `sysinfo`
(`src/hbbs_http/sync.rs`), e il server può rispondere con una `strategy` che **scrive opzioni
sull'host**. Hook in `src/clanto/server_sync.rs`:

- **In uscita** hostname e nome utente raccolti dal sistema non partono, salvo
  `clanto-share-device-identity=Y` (GPO `ShareDeviceIdentity`). Le etichette impostate
  dall'amministratore (`preset-device-name`, `preset-device-username`) partono sempre.
- **In entrata** la `strategy` non può mai scrivere le chiavi di `SERVER_PROTECTED_KEYS` (2FA,
  allowlist, controller fidati, postura, filigrane, blocco impostazioni, conservazione dei report,
  policy dei file). Con `clanto-lock-settings` attivo viene ignorata, salvo le chiavi di
  `SERVER_MANAGED_KEYS`: i servizi del forwarding (`clanto-forward-services-host`, `-controller`)
  si impostano sempre da GPO e da API. Se la GPO imposta la stessa chiave vale la GPO
  (`OVERWRITE_SETTINGS`); da soli non aprono nulla senza `clanto-forward-enabled`, che resta
  protetta.
  Un server compromesso non può quindi abbassare la sicurezza degli host. Il pannello di gestione
  dovrà usare un canale firmato, non questo.

## Forwarding

Canali nei due sensi dentro una **sessione di controllo remoto** autorizzata, separati dal port
forward di upstream (che resta spento e non viene toccato). Codice in `src/clanto/forwarding/`,
interfaccia in `flutter/lib/clanto/forwarding.dart` (voce "Forwarding" nel menu della sessione,
scheda "Forwarding" nelle impostazioni di sicurezza).

- **Protocollo.** Un solo messaggio `ClantoForward` (campo `1000` di `Message`, in
  `libs/base/protos/message.proto`, da conservare nei porting). I frame dei canali sono
  `PortForwardChannel` di upstream incapsulati: `engine.rs` riusa senza modificarle finestre, crediti e
  `run_channel` di `crate::port_forward_mux`. Il motore è simmetrico: id dispari aperti dal
  controller, pari dall'host. Un tipo di canale per funzione: oggi porte TCP e la webcam del controller; audio e USB si
  aggiungono come nuovi tipi.
- **Direzioni.** `to_host`: il controller ascolta su `127.0.0.1` e raggiunge una destinazione dalla
  parte dell'host. `to_controller`: su richiesta del controller l'host ascolta su `127.0.0.1` e porta
  le connessioni a una destinazione dalla parte del controller (host `clanto-listen`, porta =
  listen_id). Nessun listener è mai aperto sulla rete, solo in loopback; sull'host solo porta libera
  scelta dal sistema o ≥ 1024. Eliminare un forward chiude anche le connessioni già aperte.
- **Finestra in sessione** (`forwarding.dart`). Si sceglie un servizio del catalogo oppure una
  destinazione personalizzata in tre campi: il PC stesso (`localhost`) o un altro dispositivo della
  sua rete (IP o nome), la porta del servizio, la porta da aprire in loopback dall'altra parte
  (0 = automatica). La policy dell'altro lato decide comunque se la destinazione è ammessa.
  Nelle impostazioni le destinazioni ammesse si aggiungono allo stesso modo, una riga per voce
  (porta singola, intervallo `N-M` o `*`); l'opzione salvata resta l'elenco `indirizzo:porte`, e le
  voci avanzate impostate da GPO/MDM/API (CIDR, `*`) compaiono e si possono togliere.
- **Permesso della connessione.** Se all'ID del controller è assegnato un profilo di permessi, l'host
  accetta canali e listener solo se il profilo concede «Tunnel», e la webcam (nei due sensi) solo se
  concede «Fotocamera»: un profilo «Sola visualizzazione» non apre né l'una né l'altra. Non conta l'opzione globale `enable-tunnel`, che è il port
  forward di upstream (spento di default). Il valore arriva con ogni messaggio
  (`clanto_forward_profile` e `clanto_camera_profile` in `connection.rs`, `host::Permissions`).
- **Policy** (`policy.rs`, tutte gestibili via GPO, protette dalla `strategy` del server). Ogni lato
  decide solo su ciò che espone:
  - `clanto-forward-enabled` (master, spento) su ogni PC;
  - `clanto-forward-services-host` / `-controller`: servizi del catalogo spuntati, voci
    `id=destinazione` separate da `;` (dalla parte dell'host / del controller);
  - `clanto-forward-custom-host` + `-targets`: destinazioni personalizzate dalla parte dell'host;
  - `clanto-forward-reverse`: l'host può aprire listener per `to_controller`;
  - `clanto-forward-custom-controller` + `-targets`: cosa il controller espone.
  Elenchi `indirizzo:porte` (nome, IP, `[IPv6]`, CIDR o `*`; `N`, `N-M` o `*`); `localhost` vale
  anche per 127.0.0.1 e ::1. Un nome viene risolto e ci si collega solo agli indirizzi verificati;
  si risolve solo se una regola ammessa potrebbe corrispondere (nome uguale, rete o `*` sulla porta),
  così un peer non fa interrogare il DNS per nomi qualsiasi.
- **Catalogo** (`catalog.rs`): `https://clanto.it/api/clantodesk-forward-catalog.json`, JSON
  semplice `{"version": 1, "services": [...]}` senza firma (esempio in
  `api/forward-catalog.example.json`). Aiuta a scegliere e non autorizza: le impostazioni mostrano
  una casella per servizio e spuntarlo salva `id=destinazione` nell'opzione della sua direzione.
  L'host autorizza solo su quelle voci, senza scaricare nulla, quindi un elenco pubblicato
  modificato non allarga ciò che è già attivo. Le due opzioni si impostano anche da GPO
  (`ForwardServicesHost`, `ForwardServicesController`) e dalla `strategy` dell'API server; una voce attiva non più nell'elenco (o con altra
  destinazione) resta visibile per toglierla. Id ammessi: lettere, cifre, `-`, `_`, `.`.
  Aggiornamento ogni ora, uno alla volta, solo dalla UI.
- **Forwarding webcam** (`media.rs`, `camera_capture.rs`, `camera_receiver.rs`, `camera_sink.rs`),
  separato dal forwarding rete: opzione propria `clanto-forward-webcam` (spenta, va accesa su
  entrambi i PC) e menu ⚡ «Forwarding webcam» (`forwarding.dart`, `_WebcamDialog`). Due modalità:
  - **Dare la propria webcam** (`clanto-forward-webcam-send` sul tecnico): il controller cattura e
    invia, l'host la mostra come camera virtuale. Flussi con id dispari.
  - **Usare la webcam del PC controllato** (`clanto-forward-webcam-receive` sul tecnico, solo
    desktop): `ClantoCameraRequest` (campo 10) → l'host passa la richiesta alla finestra di
    gestione (`ipc::Data::ClantoCameraRequest`, evento `clanto_camera_request`, dialogo
    `clantoCameraConsentDialog`); senza risposta entro 60 s è negata. Con il consenso
    `ClantoCameraGrant` (11) porta l'elenco delle webcam dell'host, il tecnico sceglie e
    `ClantoCameraPull` (12) avvia l'invio dall'host (flussi con id pari) verso la camera virtuale
    del controller. Il consenso vale per la sessione. Host Android e iOS: non supportato (nessuna
    richiesta all'utente).
  Messaggi `ClantoMedia*` dentro `ClantoForward` (campi 5-9). La cattura usa nokhwa (Media
  Foundation, V4L2, AVFoundation; formato più grande fino a 1280x720), codifica VP8 con il codec di
  scrap e al massimo 3 frame non confermati: se il collegamento rallenta salta i frame prima di
  codificarli, un errore di decodifica chiede un keyframe. Chi riceve (`camera_receiver`, chiave
  `Host(conn)` o `Controller(sessione)`) decodifica in un thread proprio; una sola camera virtuale per
  PC. Il menu «View camera» di RustDesk è tolto dall'interfaccia del controller.
  - Linux: `clanto-forward-camera-device` sceglie il dispositivo v4l2loopback.
  - Linux: scrive in YUYV su un dispositivo v4l2loopback già presente (non lo installiamo).
    Il Flatpak concede solo `--device=dri`: per la webcam (invio o ricezione) l'amministratore la
    abilita in modo esplicito con `flatpak override --device=all it.clanto.clantodesk`; di default
    la sandbox non si allarga.
  - Windows 11: `libs/clanto-vcam`. Il servizio installato registra in HKLM la media source
    `clanto_vcam.dll` (crate `libs/clanto-vcam/dll`, copiata da `build.py` accanto all'eseguibile),
    crea la camera con `MFCreateVirtualCamera` per la durata del flusso e scrive i frame NV12 in una
    memoria condivisa `Global\ClantoDeskVirtualCamera` (lettura solo per LocalService, cioè il Frame
    Server). La media source adatta il frame a 1280x720 o 640x480 con bande nere. Nessun driver.
    Con la versione portable (senza servizio) la camera virtuale non è disponibile.
  - macOS 13+ come host: Camera Extension `it.clanto.clantodesk.camera`
    (`flutter/macos/CameraExtension/`), con uno stream di uscita per le applicazioni e uno stream
    sink in cui l'host scrive (`libs/clanto-vcam/src/mac.rs`, API C di CoreMediaIO, 1280x720 NV12
    con bande nere). La CI la compila, la firma e la incorpora in
    `Contents/Library/SystemExtensions` con `.github/scripts/build-macos-camera-extension.sh`, solo
    se esistono i secret `MACOS_APP_PROVISIONING_PROFILE_BASE64` e
    `MACOS_CAMERA_PROVISIONING_PROFILE_BASE64`: profili Developer ID per `it.clanto.clantodesk` e
    `it.clanto.clantodesk.camera`, con capability System Extension (app) e App Group
    `<TEAM>.it.clanto.clantodesk` (entrambi). Senza secret l'app resta com'era. L'utente attiva
    l'estensione dal pulsante «Attiva la camera virtuale» (impostazioni, Forwarding;
    `OSSystemExtensionRequest` in `AppDelegate.swift`) e la approva in Impostazioni di Sistema; l'app
    deve stare in `/Applications`. Type-check Rust per `aarch64`/`x86_64-apple-darwin` in Docker; Swift
    compilato solo dalla CI; prova funzionale su un Mac.
  - macOS come controller: entitlement `com.apple.security.device.camera` e
    `NSCameraUsageDescription`.
  - iOS come controller: AVFoundation in `flutter/ios/Runner/ClantoCamera.swift` (1280x720 NV12,
    diritta in verticale) chiama `clanto_camera_push_nv12` (dichiarata in `ClantoCamera.h`, inclusa
    dal bridging header). `NSCameraUsageDescription` in Info.plist; la fotocamera funziona solo con
    l'app in primo piano. Compilato solo dal job iOS della CI; prova su iPhone.
  - Android come controller: il telefono fa da webcam del tecnico. Camera2 in
    `ClantoCamera.kt` (posteriore o anteriore, fino a 1280x720, raddrizzata con la rotazione del
    sensore per il telefono in verticale) passa i frame YUV_420_888 a Rust via JNI
    (`camera_capture_mobile.rs`, conversione in `media::android420_to_i420`). Permesso `CAMERA`
    nel manifest, chiesto solo al primo «Avvia»; `android.hardware.camera` non obbligatoria, quindi
    Play non esclude i dispositivi senza camera. Va dichiarato nella sezione Sicurezza dei dati di Play.
  - Prove in Docker: VP8 andata e ritorno, conversioni, ABI v4l2, buffer condiviso e adattamento
    del frame (`cargo test -p clanto-vcam`), type-check dei crate Windows per
    `x86_64-pc-windows-msvc`. La camera virtuale di Windows va provata su un PC Windows 11 reale.
- **Audit**: 61170 canale aperto, 61171 canale negato, 61172 listener aperto, 61173 listener negato,
  61174 webcam aperta o consenso dato, 61175 negata (anche consenso rifiutato), 61176 chiusa (frame e durata); il campo `direction` vale `to_controller` per la webcam dell'host. Gli eventi di
  rifiuto sono limitati a 30 al minuto per connessione; oltre, solo nel log.
- **Robustezza webcam.** L'host crea la camera virtuale e decodifica in un thread proprio, mai in
  quello della connessione; legge le dimensioni del keyframe VP8 prima di decodificarlo e scarta
  frame oltre 2 MB; aspetta fino a 3 s che la camera di un flusso appena chiuso si liberi. Il
  controller usa un id nuovo per ogni avvio e chiude il flusso se la camera non manda frame per 5 s.
  Su Windows l'elenco delle webcam gira in un thread nuovo con COM in STA, come nokhwa. La media
  source Windows torna al nero se il servizio non scrive frame per 2 s.
- **Piattaforme.** Controller (porte e webcam): Windows, macOS, Linux, Android e iOS. Su iOS l'app
  viene sospesa in background: un forward resta attivo solo con ClantoDesk in primo piano (su iPad
  anche in Split View), quindi è utile soprattutto verso il controller o per brevi verifiche. Host:
  tutti tranne iOS; la webcam del tecnico si riceve su Linux, Windows 11 e macOS 13+.
- **Prova.** `docker/e2e/run.sh`, scenari 14-22, con host Linux su Xvfb: le sessioni di controllo
  remoto enumerano i monitor e senza display l'host rifiuta il login. Gli scenari 20-22 inviano un
  pattern di prova VP8; l'host di prova usa un sink nullo (`CLANTO_E2E_CAMERA_SINK=null`, solo nelle
  build di debug e senza dispositivo scelto).

## Accesso solo da dispositivi protetti

Opzione host `clanto-require-protected-device` (default `N`), nella sezione «Sicurezza in ingresso»
delle impostazioni. Con `N`, se non è richiesta nemmeno la filigrana del controller, il login è
identico a upstream: l'host non chiede nulla e non fa chiamate di rete. Con `Y`, dopo l'allowlist per
ID e prima di qualunque verifica della password, l'host manda la postura
dichiarata dal controller a `https://clanto.it/api/clantodesk-posture.php` e rifiuta se la risposta
non è `ok`. **Fail-closed**: API irraggiungibile, timeout di 5 s, JSON non valido, postura o firma
mancanti valgono rifiuto. Il PC host deve quindi raggiungere `clanto.it`.

Se `clanto-require-controller-watermark=Y` ma il dispositivo protetto non è richiesto, nello stesso
punto del login l'host usa la modalità `signature`: verifica richiesta, configurazione, firma,
coerenza e versione revocata, ma non applica le regole della policy di postura. Serve a riconoscere
il client ClantoDesk ufficiale senza imporre la postura completa ed è ugualmente fail-closed. Con
entrambe le opzioni attive viene eseguita una sola richiesta, quella completa; con entrambe spente
non viene eseguita alcuna richiesta.

I rifiuti sono classificati prima di essere inviati al controller: versioni precedenti alla 1.5.0
o revocate sono `outdated`, postura/firma assente su versioni recenti e firme non valide sono
`unofficial`, timeout, errori di rete e risposte JSON non valide sono `unreachable`, mentre le
regole di postura non rispettate sono `policy`. Il controller riceve un testo già tradotto
dall'host e mai gli identificatori interni `core:*`; questi ultimi restano nel log, nell'audit e
nel report locale insieme alla classe del rifiuto. Il messaggio `outdated` ha un prefisso Unicode
invisibile: i client precedenti lo mostrano normalmente, quelli dalla 1.5.0 lo rimuovono e offrono
il pulsante «Aggiorna», usando il download/installer desktop o la pagina Play Store su Android.

- **Controller.** `src/clanto/posture.rs`: `collect()` raccoglie le chiavi di `POSTURE_KEYS` (sistema,
  patch, antivirus, firewall, firmware, rete...), `unknown` dove non arriva senza privilegi o entro il
  tempo (3 s). Su Windows la raccolta è nativa, senza PowerShell (`libs/clanto-win/src/posture.rs`):
  registro, WMI solo per Security Center, BitLocker e VBS, TBS per il TPM (anche senza privilegi),
  IP Helper e WLAN per la rete. `os_ubr` è la revisione (es. 19045.7807) e `os_patch_days` i
  giorni dall'ultimo aggiornamento cumulativo installato (pacchetti CBS `RollupFix`): la policy decide se Windows è supportato da quanto è recente l'ultimo cumulativo,
  non da una tabella di build e date. Il risultato resta in cache per 10 minuti; `prewarm()` parte in
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
  nell'audit.
- **Policy.** Vive sul server, in `posture-policy.json` accanto alla PHP: regole, versioni revocate,
  gruppi di host e tabella EOL (oggi solo Linux: Windows si giudica da `os_patch_days`). Le fasce
  orarie non sono qui ma sull'host (`clanto-access-hours`). Si cambia senza rilasciare il client. Una regola malformata
  blocca tutti gli accessi (`core:policy:<id>`): provarla prima di pubblicarla.
- **Server.** I file stanno in `api/` di questo repo (`clantodesk-posture.php`, `posture-policy.json`,
  insieme a `clantodesk.php` dei download) e si caricano così come sono su `clanto.it/api/`. Il
  segreto non è nel repo: la PHP legge `CLANTO_POSTURE_MASTER` dall'ambiente oppure, se manca, dal file
  `<sito>/private/clanto-posture.secret` (su ISPConfig `private/` sta accanto a `web/` e nginx non lo
  serve). Il file contiene solo il valore, lo stesso del secret GitHub, leggibile dall'utente del sito e
  non dal mondo. Senza segreto la PHP risponde `core:config` e rifiuta ogni accesso.
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

## Filigrana

È sempre il PC controllato a decidere. Non esiste un'opzione locale del
controller: con `clanto-require-controller-watermark=Y` l'host comunica il
requisito tramite la chiave omonima in `PeerInfo.platform_additions`. La
filigrana diagonale compare nel controller soltanto per desktop remoto e
visualizzazione fotocamera; non compare per trasferimento file, terminale o
port forwarding/RDP. È un widget Flutter separato dalla texture, non intercetta
l'input e resta sopra l'immagine durante zoom, scorrimento e cambio monitor.

Le righe diagonali si alternano. La prima è `<nome> · ID <id> · <codice
sessione>`: il nome è l'utente dell'account oppure, se manca, l'utente di
sistema senza dominio (`DOMINIO\utente` e `utente@dominio` diventano `utente`).
La seconda è `→ <hostname> · dd/MM/yyyy HH:mm`. Le parti non disponibili e il
relativo separatore vengono omessi. Testo bianco al 12% con contorno al 12%,
volutamente rado: deve restare leggibile in una foto senza coprire il lavoro.

Quando la filigrana è richiesta, l'host accetta soltanto un controller con la firma HMAC della
release ufficiale, verificata da `clanto.it` prima della password. Un client semplicemente
ricompilato non possiede la chiave di release e viene rifiutato. Resta possibile modificare un
binario ufficiale conservandone la chiave incorporata: la filigrana è quindi un deterrente che
rende riconoscibile chi osserva la sessione negli screenshot e nelle fotografie, non una protezione
tecnica contro la modifica del binario o acquisizioni effettuate con software o dispositivi esterni.
Con questa opzione l'host forza inoltre a `false` il permesso di registrazione e ignora i tentativi
di riattivarlo dalla finestra della connessione; il controller ufficiale nasconde il relativo comando.

L'opzione host `clanto-host-watermark` (predefinita `N`) mostra invece sul PC
controllato una fascia semitrasparente su ogni schermo per tutta la durata di
almeno una sessione desktop o fotocamera autorizzata. È sempre una sola riga,
centrata in una fascia alta una riga, con opacità circa 55%:
`● Sessione remota · <tecnico> (<ID>) · <codice sessione> · dalle HH:MM`,
riferita alla sessione più recente; con più sessioni aggiunge ` · +N`. Tipo di
accesso, permessi, utenti e PC del controller restano nel report sessioni e
nell'audit, non nella fascia. Scompare alla chiusura dell'ultima sessione. Non si attiva per sessioni di solo trasferimento file, terminale o
port forwarding/RDP. Il valore resta separato da
`clanto-require-controller-watermark`, viene applicato anche tramite GPO e il
controllo nell'interfaccia è bloccato quando la policy è configurata.

Per ogni connessione autorizzata l'host genera con un generatore crittografico
un codice breve `S-XXXX-XXXX`. Il codice viene inviato sempre al controller,
anche quando nessuna filigrana è attiva, ed è scritto come `session_code` nei
record di audit, nell'estensione CEF/SIEM e nel report locale delle sessioni
(inclusa l'esportazione CSV). In questo modo una schermata può essere correlata
al record amministrativo senza mostrare l'indirizzo IP.

L'identità estesa del controller viene richiesta dopo l'autorizzazione e solo
quando l'host ha attivato `clanto-host-watermark` oppure
`clanto-require-controller-watermark`. Il messaggio contiene esclusivamente
utente account, utente di sistema e nome PC: sono i dati personali minimi utili
per attribuire visivamente la sessione e vengono conservati nel record della
sessione e nell'audit. L'IP rimane disponibile ai controlli e ai log
amministrativi esistenti, ma per minimizzazione non appare in nessuna delle due
filigrane.

Dove il sistema consente un overlay globale, la finestra è gestita solo da
ClantoDesk: non ha bordi o controlli, non riceve mouse o tastiera, non prende il
focus, non può essere chiusa o spostata dall'utente locale o dal controller e
non compare nel selettore delle applicazioni, nella taskbar o nel Dock.

Le garanzie della filigrana host dipendono dalla piattaforma:

* su Windows è una finestra nativa layered, trasparente all'input, topmost,
  tool window e no-activate, creata nel processo dell'utente interattivo anche
  quando la finestra CM è nascosta. Non compare in Alt-Tab o nella taskbar ed è
  esclusa dalla cattura tramite `WDA_EXCLUDEFROMCAPTURE`;
* su macOS è una `NSWindow` borderless sopra screensaver e status bar,
  `ignoresMouseEvents`, senza focus né Dock, visibile in tutti gli Space ed
  esclusa dalla cattura con `sharingType = .none`;
* su Linux X11 è una finestra override-redirect sempre in primo piano, senza
  area di input grazie a XShape. X11 non offre qui un'esclusione affidabile
  dalla cattura, quindi la filigrana può comparire nell'immagine trasmessa;
* su Linux Wayland un client non può imporre un overlay globale sempre in primo
  piano. ClantoDesk usa finestre persistenti di tipo notifica, una per monitor,
  ma il compositor può ignorarne posizione e livello; non può quindi garantire
  overlay su ogni monitor, click-through globale o esclusione dalla cattura;
* su Android host usa `TYPE_APPLICATION_OVERLAY` con
  `FLAG_NOT_TOUCHABLE | FLAG_NOT_FOCUSABLE`, tramite il permesso
  `SYSTEM_ALERT_WINDOW` già presente nel manifest principale. Il manifest
  ridotto della build solo controller non viene modificato. L'interfaccia
  richiede l'autorizzazione overlay prima di abilitare l'opzione; se la policy
  viene impostata senza passare dall'interfaccia, l'amministratore deve anche preautorizzare
  l'app-op overlay Android, altrimenti il sistema non permette di mostrarla.

Il collegamento o la rimozione di un monitor mentre una sessione è già attiva
viene recepito al successivo ingresso/uscita di sessione (o al riavvio del CM):
la filigrana non mantiene un ulteriore watcher permanente dei display.

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

## Firma del codice

Identità di firma fissata al build e verificata con API native (niente PowerShell):

- **Windows: EKU di Azure Trusted Signing.** Ogni certificato Azure di Clanto porta l'EKU
  `1.3.6.1.4.1.311.97.198846753.677989547.281320684.501451963`: pubblico, stabile anche se il
  certificato ruota (dura 3 giorni) e legato alla verifica d'identità di Clanto, quindi non
  ottenibile da un'altra società con lo stesso nome. La CI lo passa al build come
  `CLANTO_SIGNING_EKU` solo se la firma Azure è configurata, e dopo la firma verifica che ogni
  binario lo porti (passo «Verify signatures and Clanto EKU»): un cambio d'identità ferma il build.
- **Exe.** `flutter/windows/runner/clanto_integrity.cpp`, prima di caricare `librustdesk.dll`:
  ogni `.exe`/`.dll` della cartella dell'eseguibile (installazione o copia portable estratta) deve
  avere firma valida con l'EKU, oppure Microsoft (anche tramite catalogo, es.
  `RuntimeBroker_rustdesk.exe`). Altrimenti evento 61180 nel registro Applicazione, messaggio
  «Eseguibile manomesso» (non in sessione 0 né in CI) e uscita: servizio, server e interfaccia
  non partono. `WinVerifyTrust` senza rete: niente revoca, solo URL in cache.
- **librustdesk.** `verify_own_exe()` (`src/clanto/signature.rs`, tramite `libs/clanto-win`)
  all'inizio di `core_main()` verifica a sua volta l'exe: modificare uno solo dei due non basta.
- **Aggiornamenti.** L'installer scaricato si esegue solo con firma valida e lo stesso EKU.
  Installazione e aggiornamento cancellano dalla cartella gli `.exe`/`.dll` che il nuovo
  pacchetto non contiene più (`stale_binaries_cmd`, hook in `copy_exe_cmd`): un plugin tolto
  da una versione precedente (es. `file_selector_windows_plugin.dll`) resterebbe senza la firma
  attuale e bloccherebbe l'avvio.
- **macOS.** Team Apple `DCP3L7L7BW` (`CLANTO_SIGNING_TEAM` al build, solo con i secret di firma):
  `SecStaticCodeCheckValidity` sul bundle, codice annidato compreso, con il requisito
  `anchor apple generic and certificate leaf[subject.OU] = "DCP3L7L7BW"`. Bloccante per
  `--service`/`--server`; l'interfaccia lo fa in background e mostra l'errore prima di chiudersi
  (`flutter/lib/clanto/tamper_check.dart`). Vale anche per gli aggiornamenti.
- **Limite.** Chi è amministratore può modificare entrambi i controlli. Il blocco vero lo fa
  Windows con App Control (WDAC) su quell'EKU.

## Aggiornamenti

- **Windows, pulsante «Aggiorna».** Con ClantoDesk installato la UI chiede l'aggiornamento al
  processo `--server` (SYSTEM) via IPC (`clanto-update-now`, `src/clanto/service_update.rs`) e
  mostra lo stato (`clanto-update-state`: controllo, download, installazione, errore): nessuna
  richiesta di amministratore, anche per un utente standard. Il servizio usa il percorso
  dell'aggiornamento automatico (`updater.rs`): solo una versione più recente pubblicata nelle
  release Clanto, firma con l'EKU Clanto, installazione con `--update` (o `msiexec /qn` per
  l'MSI). La richiesta manuale non aspetta l'intervallo di 10 minuti né la fine delle sessioni.
  Il servizio scarica in `<config del servizio>\update`, che gli utenti non possono scrivere: un
  file preparato o sostituito da un utente tra verifica e avvio non è possibile. Se il servizio
  non risponde (portable, servizio fermo) o l'aggiornamento fallisce, resta il percorso upstream
  con download nella UI e UAC («Aggiorna come amministratore»).
- **Windows, automatico.** `allow-auto-update` («Aggiornamento automatico» nelle impostazioni,
  GPO `AutoUpdate`, spento di default): il servizio controlla 30 s dopo l'avvio e poi ogni giorno,
  e installa da solo quando non ci sono sessioni attive (altrimenti riprova dopo 30 minuti).
- **macOS.** Pulsante «Aggiorna»: download del DMG nella UI, estrazione, verifica del team Clanto e
  sostituzione del bundle con la password di amministratore (`osascript`). L'aggiornamento
  automatico come root (`start_auto_update_macos`) c'è ma è spento (`MACOS_AUTO_UPDATE_ENABLED`)
  finché non è provato su VM.
- **Linux.** Nessun aggiornamento dall'app: la scheda «Nuova versione» apre la pagina della
  release; i pacchetti si aggiornano con il gestore della distribuzione o reinstallando.

## Disinstallazione (Windows)

`clantodesk.exe --uninstall` (Programmi e funzionalità, collegamento «Disinstalla» anche nell'MSI)
chiede prima di partire (`src/clanto/uninstall.rs`, hook in `core_main.rs`):

- **Sì, mantieni la configurazione:** toglie programma, servizio, collegamenti, firewall,
  stampante e driver (`get_uninstall()` di upstream); restano ID, password, rubrica e impostazioni.
- **No, cancella tutto:** in più cancella, per ogni profilo in `ProfileList` e per i profili di
  servizio (`LocalService`, `systemprofile`), `AppData\Roaming\ClantoDesk` (configurazione,
  chiavi, log, report, quarantena), `AppData\Local\ClantoDesk`, i dati di Flutter
  (`AppData\Roaming\<CompanyName>\ClantoDesk`, anche con i CompanyName delle versioni
  precedenti) e le chiavi `HKLM\SOFTWARE\ClantoDesk` ed `EventLog\Application\ClantoDesk`. La
  cartella dell'utente corrente si cancella dopo l'uscita del processo, che tiene aperto il log.
  Restano le policy `HKLM\Software\Policies\Clanto`: le gestisce l'amministratore.
- **Annulla:** nessuna modifica.

Fino alla 1.3 ClantoDesk si chiamava «RustDesk» in tutto. Se trova un'installazione o dei dati
«RustDesk» li classifica: eseguibile firmato da Clanto (`signed_by_clanto`) o configurazione con
un server Clanto fissato al build = vecchia ClantoDesk, rimossa in automatico (i dati solo con
«cancella tutto»); altrimenti RustDesk ufficiale, rimosso solo se l'utente conferma la seconda
domanda. Per toglierlo si esegue prima il suo `rustdesk.exe --uninstall` (sa gestire anche MSI,
stampante e driver), poi i resti: servizio, chiave di disinstallazione, `HKCR\rustdesk`,
firewall, collegamenti, cartella.

Senza finestra (sessione 0, CI o opzione esplicita): `--keep-config` (predefinito), `--purge`,
`--remove-rustdesk` (un RustDesk ufficiale si toglie solo con questa). MSI:
`msiexec /x <prodotto> CLANTO_PURGE=1 [CLANTO_REMOVE_RUSTDESK=1]` esegue
`clantodesk.exe --clanto-purge-data` come SYSTEM prima di `RemoveFiles`; la disinstallazione MSI da
Programmi e funzionalità chiama `msiexec` direttamente e mantiene la configurazione.

## Debito noto

- `res/msi/Package/License.rtf` (italiano e inglese) non è un EULA e non deve
  diventarlo: la AGPL-3.0 (§10) vieta restrizioni ulteriori sull'uso del software.
  Condizioni contrattuali (SLA, uso dei server Clanto, responsabilità) vanno nei
  termini di servizio dell'infrastruttura, pubblicati a parte e richiamati qui con
  un link quando esisteranno.

`.github/workflows/playground.yml` di upstream è stato rimosso: era disabilitato e
pubblicava artefatti `rustdesk-*`. Nei porting da upstream le sue modifiche si
ignorano.

Allo stesso modo `docs/SECURITY*.md` di upstream (contatto `info@rustdesk.com`) sono stati rimossi:
vale `SECURITY.md` in radice. Nei porting non vanno reintrodotti.
