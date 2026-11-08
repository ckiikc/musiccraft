# MusicCraft per Android — 0.3.0

Prima versione nativa Android di MusicCraft. Progetto separato dall’app macOS, senza modifiche al suo profilo o al suo motore audio. Non è una release ufficiale di ArtCraft.

## Funzioni

- Libreria musicale locale indicizzata da Android (MediaStore), su memoria condivisa e schede rimovibili indicizzate: brani, album, artisti e ricerca.
- Riproduzione tramite Media3/ExoPlayer in un unico `MediaLibraryService`, condiviso fra telefono, notifica, controlli di sistema e Android Auto.
- Play/pausa, precedente/successivo, seek e sequenza automatica. L’audio continua quando la schermata dell’app viene chiusa; audio focus e scollegamento delle cuffie sono gestiti dal player.
- Copertina, titolo, artista e album; aggiornamento manuale della libreria. Nessun invio dei dati musicali a server esterni; condivisione opzionale sulla rete locale tramite HTTPS.
- Catalogo e ricerca esposti sia ai client Media3 sia al protocollo `MediaBrowserService` usato da Android Auto. Le connessioni sono controllate da `MediaSession`; sono ammessi l’app stessa, il controller della notifica e i controller considerati attendibili dal sistema.
- Ricerca vocale tramite sessione multimediale e azione `MEDIA_PLAY_FROM_SEARCH`.

Interfaccia iniziale in italiano. Vinile/VU meter, FTP/NAS e importazione di cartelle tramite SAF restano fuori da questa versione. I formati effettivamente riproducibili dipendono anche dai decoder del dispositivo. La libreria mostra l’audio condiviso indicizzato da Android, non i file privati di altre applicazioni.

## Cerca, Playlist e Player

La barra inferiore distingue tre viste. **Cerca** esplora e filtra la libreria senza cambiare la coda: ogni brano offre Riproduci ora, Riproduci dopo, Aggiungi in coda, Aggiungi a playlist e Valuta. La sostituzione di una coda esistente richiede conferma. **Playlist** contiene liste salvate e mix dinamici; **Player** ha una vista autonoma con copertina, seek e comandi. Ruotando il telefono, il Player dispone copertina e controlli affiancati; il servizio mantiene l’audio durante la ricreazione della schermata. Dalle altre viste il mini player riapre il Player completo.

Dal Player si apre la **Coda** per spostare, rimuovere, riprodurre o salvare i brani. Coda e posizione vengono salvate periodicamente e ripristinate in pausa dopo il riavvio del processo; non viene avviato audio automaticamente. I file non più presenti vengono esclusi dal ripristino.

### Playlist dinamiche

Otto modelli iniziali e un editor di criteri: brani, album o artisti; genere; anno o intervallo/decennio; voto da 0 a 5; testo; già ascoltati, mai ascoltati e gruppi ascoltati in parte. Ordinamento casuale, conteggio totale/medio, voto totale/medio, numero di brani votati o ascolto recente. Prima dell’avvio è disponibile un’anteprima. I filtri sui metadati e sui voti selezionano i brani prima del raggruppamento; gli album/artisti sono valutati sul sottoinsieme selezionato. Anni sconosciuti sono esclusi quando si imposta un limite sugli anni.

Il mix aggiunge un numero limitato di brani in anticipo e conserva al massimo venti elementi precedenti nella coda. Evita duplicati nello stesso ciclo e fra i brani già in attesa; terminato il ciclo può ricominciare. I mix di brani mai ascoltati si esauriscono. **Interrompi aggiunta automatica** ferma il generatore e conserva la coda esistente. Le modifiche ai criteri si applicano al prossimo avvio del mix.

Un ascolto conta dopo 30 secondi effettivi o metà brano, se più breve (minimo un secondo); pause e seek non aggiungono tempo. Voti, statistiche, regole e playlist sono privati dell’installazione. Il comportamento si ispira alle playlist dinamiche di Lyrion, senza collegamento a un server LMS né importazione delle sue statistiche o replica di tutti i suoi preset. I mix sono disponibili anche nella navigazione MediaBrowser per Android Auto; crearli e modificarli richiede l’interfaccia del telefono.

## Condivisione diretta con il computer

Disponibile per ogni installazione, senza IP o password preimpostati: dal menu **⋮** attivare **Condividi con il computer**, scegliere **Abbina computer** e inserire le quattro cifre in MusicCraft desktop 0.10.49 o successivo, che trova il telefono automaticamente. Confermare poi **Autorizza** sul telefono. Accesso in sola lettura, sospeso mentre il telefono riproduce audio, con notifica e revoca degli abbinamenti. Non occorre un’app FTP. Procedura, protocollo, limiti e sicurezza sono documentati in [SHARING.md](../docs/SHARING.md).

## Requisiti e compilazione

Android 8.0/API 26 o successivo. Build con SDK/target 36, JDK 21, Gradle 8.14.3, AGP 8.10.1, Kotlin 2.2.20 e Media3 1.11.1. Le versioni sono fissate nel progetto.

Impostare `ANDROID_HOME` o creare un `local.properties` ignorato con `sdk.dir=...`, quindi:

```sh
./gradlew :app:assembleDebug :app:testDebugUnitTest :app:lintDebug
```

APK: `app/build/outputs/apk/debug/app-debug.apk`. L’APK debug è firmato per i collaudi locali; non è un pacchetto pubblicato su Google Play. La chiave privata e i percorsi locali non vanno pubblicati. Per aggiornare un’installazione servono lo stesso certificato e un versionCode compatibile.

## Prova sul telefono

1. Installare l’APK e aprire MusicCraft.
2. Premere **Consenti accesso alla musica** e autorizzare audio/musica nella richiesta Android. La richiesta delle notifiche è distinta: la sua negazione non sostituisce il permesso sulla libreria.
3. Aprire un brano, premere Home o chiudere la schermata, poi controllare notifica e avanzamento al brano successivo.
4. Se la libreria è vuota, verificare che i file siano nella memoria condivisa e indicizzati da Android, quindi scegliere **⋮ → Aggiorna libreria**.

Non concedere i permessi del telefono via script: le richieste vanno approvate dall’utente sul dispositivo. I test automatici che creano/rimuovono fixture si rifiutano di girare su telefoni reali.

## Android Auto

Il manifest dichiara l’app come servizio musicale Android Auto. Concedere prima il permesso audio nell’app sul telefono. Una build installata direttamente tramite APK può richiedere l’opzione **Origini sconosciute** nelle impostazioni sviluppatore di Android Auto; non va confusa con la schermata Android per installare APK. Per la distribuzione ordinaria e ulteriori test sono disponibili Google Play e i canali interni.

Il collaudo dei protocolli multimediali non certifica da solo la comparsa nel launcher dell’automobile. Il display e i comandi di un’auto reale o del Desktop Head Unit richiedono una prova distinta.

Fonti ufficiali:

- https://developer.android.com/media/implement/surfaces/cars
- https://developer.android.com/training/cars/testing
- https://developer.android.com/media/media3/session/background-playback

## Architettura

`Catalog.kt` legge MediaStore su un worker, conserva una snapshot e serve identificativi stabili e pagine con confini controllati. `PlaybackService.kt` possiede l’unico player e risolve gli ID delle richieste contro la libreria: i client non possono imporre URI arbitrari. `PlaybackCoordinator.kt` gestisce coda, ascolti e generatore dinamico nello stesso servizio. `ListeningStore.kt` salva snapshot con AtomicFile su un worker; un file illeggibile viene conservato e segnalato. `SmartRules.kt` contiene la selezione pura e testabile. `MainActivity.kt` usa un MediaBrowser/MediaController e non apre un secondo player. Il rilascio del controller della schermata non rilascia il servizio in riproduzione.

## Verifiche

Consultare `VALIDAZIONE.md` per gli esiti effettivi. I test strumentali usano tre FLAC sintetici e un emulatore separato. Per eseguirli, compilare `:app:assembleDebugAndroidTest`, installare applicazione e APK test sull’emulatore, concedere lì i permessi audio/notifiche e lanciare:

```sh
adb -s SERIAL_EMULATORE shell am instrument -w -r \
  it.ckiikc.musiccraft.test/it.ckiikc.musiccraft.MusicCraftInstrumentation
```

Non installare l’APK test sul telefono personale. Il runner azzera solo lo stato MusicCraft dell’emulatore dedicato. Le fixture sono isolate in `Music/MusicCraft QA` e vengono ripulite prima della fine del test.
