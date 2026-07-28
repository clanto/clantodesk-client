# Note sul fork ClantoDesk

Questo repo è un fork di [rustdesk/rustdesk](https://github.com/rustdesk/rustdesk).
Branch di lavoro: `clnt_1.4.9`. `master` è un mirror di upstream, non ci si sviluppa.

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
[src/common.rs](src/common.rs), chiamata **dopo** `load_custom_client()` perché
anche quello può fornire server e chiave.

Per una build di sviluppo locale:

```bash
CLANTO_ALLOW_PUBLIC_SERVER=1 cargo build ...
```

Degrada il blocco a warning. Non distribuire binari compilati così.

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

## Aggiungere funzionalità senza pagarle a ogni merge

Un hook, non venti modifiche sparse: la logica in un modulo proprio, e **una riga**
di chiamata nel file upstream. Una riga aggiunta si auto-mergia quasi sempre;
trenta righe intrecciate nel corpo di una funzione upstream conflittano sempre.

Dove possibile usare i meccanismi che esistono già invece di modificare il codice:
`DEFAULT_SETTINGS` / `OVERWRITE_SETTINGS` per le opzioni, `get_app_name()` invece
dei letterali, `option_env!` per i valori iniettati al build.

## Debito noto

- `res/msi/Package/License.rtf` è un testo **provvisorio** che rimanda alla policy
  pubblicata: va sostituito con l'EULA vero, e serve la versione inglese se si
  distribuisce fuori dall'Italia.
- `.github/workflows/playground.yml` è un workflow sperimentale di upstream, con
  nomi `rustdesk-*` e riferimenti a `RustDesk.app` che non esiste più: se lanciato
  a mano pubblicherebbe artefatti con il nome sbagliato sulla release `nightly`.
