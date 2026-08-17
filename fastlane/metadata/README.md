# Metadati Store

- `android/` contiene i metadati Google Play per Fastlane Supply.
- `ios/` contiene i metadati App Store per Fastlane Deliver; usare questa
  cartella come `metadata_path` quando viene configurata la lane iOS.

L'icona Google Play deve restare un PNG 512×512 e la feature graphic un PNG
1024×500. Le note Android sono versionate con il `versionCode` in
`<locale>/changelogs/74.txt`.

Gli screenshot non vengono generati o ricostruiti artificialmente: devono
essere acquisiti dalla build 1.4.9+74 su dispositivo o simulatore, dopo avere
verificato che non mostrino ID, nomi, indirizzi o altri dati reali. Per Android
vanno collocati nelle cartelle Fastlane `phoneScreenshots/` e, se pubblicato per
tablet, `sevenInchScreenshots/` e `tenInchScreenshots/`. Gli screenshot iOS
vanno caricati in App Store Connect nelle dimensioni richieste per i dispositivi
selezionati.
