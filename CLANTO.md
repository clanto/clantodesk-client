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
| non impostata | completa | `it.clanto.clantodesk` | ClantoDesk | build storiche, desktop |

Il lato client sparisce da `initPages()` con `isIncomingOnly()`; il lato
controllato sparisce perché `RendezvousMediator::start_all` non registra il
dispositivo (`src/rendezvous_mediator.rs`), quindi **non ha un ID
raggiungibile**. Non è un'interfaccia nascosta: è l'assenza dalla rete.

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

## Debito noto

- `res/msi/Package/License.rtf` è un testo **provvisorio** che rimanda alla policy
  pubblicata: va sostituito con l'EULA vero, e serve la versione inglese se si
  distribuisce fuori dall'Italia.
- `.github/workflows/playground.yml` è un workflow sperimentale di upstream, con
  nomi `rustdesk-*` e riferimenti a `RustDesk.app` che non esiste più: se lanciato
  a mano pubblicherebbe artefatti con il nome sbagliato sulla release `nightly`.
