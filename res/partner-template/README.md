# Repo partner ClantoDesk (livello 1)

Modello del repo `github.com/clanto/<partner>`: contiene solo i dati del brand.
La build la fa il workflow «Partner build» del repo `clanto/clantodesk-client`
(CLANTO.md, «Build per i partner»), che firma con i certificati Clanto e
pubblica qui la release.

## brand.json

| Campo | Regola | Dove compare |
|---|---|---|
| `name` | 2-40 caratteri: lettere e cifre ASCII, spazio, `.`, `-` | titolo e barra in alto, testi al posto di «ClantoDesk», tray, Task Manager, barra dei menu macOS |
| `company` | 2-60 caratteri, senza virgolette | testi al posto di «Clanto» (es. «server Acme») |
| `file-prefix` | minuscole, cifre e `-`, inizia con una lettera | nomi dei file della release: `acmedesk-1.5.1-x86_64.exe` |
| `support-url` | `https://` | link «Assistenza» nelle informazioni e guide di aiuto |
| `website-url` | `https://` | link «Sito web» nelle informazioni |

## icons/

Tutti obbligatori, al massimo 2 MB ciascuno:

| File | Formato | Uso |
|---|---|---|
| `icon.png` | PNG quadrato, 512×512 consigliato | logo nell'app e tray Windows |
| `icon.ico` | ICO con 16, 32, 48, 256 px | icona di `clantodesk.exe` e dell'installer |
| `tray-icon.ico` | ICO con 16 e 32 px | tray Windows (riserva) |
| `AppIcon.icns` | ICNS | icona dell'app macOS e del Dock |
| `icon_tray_macos.png` | PNG nero su trasparente, 44×44 | barra dei menu macOS (immagine template: conta solo la trasparenza) |

## Cosa resta ClantoDesk

Servizio, cartelle di configurazione, `clantodesk.exe` installato, bundle
`ClantoDesk.app`, voce in «App installate», editore della firma, informativa
privacy e copyright del software.
