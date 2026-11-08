# Verifica 0.1.0 — 8 novembre 2026

## Build e pacchetto

Build debug completata con SDK/target 36, Media3 1.11.1 e Kotlin 2.2.20. APK firmato e verificato con apksigner (schema v2); applicazione `it.ckiikc.musiccraft`, versionCode 1, versione 0.1.0, minSdk 26. L’APK contiene le licenze e non contiene file musicali di test, dati personali o chiavi private.

Due test unitari passati: ricerca su titolo/artista/album e paginazione con input invalidi e limiti numerici. Android Lint: 0 errori, 11 warning, relativi principalmente a risorse linguistiche della prima versione italiana, wrapper aggiornabile, icona e configurazioni intenzionali del servizio pubblico/contesto applicativo. I warning non costituiscono una verifica aggiuntiva di compatibilità hardware.

## Emulatore Android 13/API 33 dedicato

Suite strumentale completata con tre FLAC sintetici e tag reali. Il runner si rifiuta di operare su telefoni fisici e pulisce solo le proprie fixture, salvo il parametro esplicito per il controllo visivo su emulatore.

Verificati:

- Lettura e righe realmente presenti nella schermata del telefono.
- Connessione Media3 e connessione attraverso il MediaBrowser Android tradizionale, root e categorie Brani/Album/Artisti, navigazione, paginazione e ricerca.
- Risoluzione degli identificativi della libreria tramite il servizio audio.
- Avvio della riproduzione, precedente/successivo, seek e pausa.
- Riproduzione che prosegue dopo `Activity.finish`, anche se nel frattempo cambia il brano.
- Sequenza automatica Gamma → Alpha → Beta e arresto alla fine, senza ripetizione.
- Clic reale su una riga dell’elenco e stato della sessione `PLAYING`; riproduzione ancora attiva dopo il passaggio alla Home.

Ispezionate schermate reali della libreria e del player; catture e log sintetici negli output ignorati del workspace.

## Telefono e Android Auto

Telefono Android 16 riconosciuto da ADB e Android Auto presente. Nessuna vecchia installazione MusicCraft Android trovata. Installazione USB della 0.1.0 riuscita dopo la conferma dell’utente. Versione e SHA-256 dell’APK installato verificati contro il pacchetto consegnato. Avvio a freddo completato (721 ms), senza crash rilevati nel processo. Nessun permesso concesso via script e nessuna modifica ai file musicali del telefono. Permesso audio successivamente concesso dall’utente sul telefono: libreria reale popolata e righe disponibili nella schermata, senza messaggi di errore del player. Titoli e artisti personali non sono stati copiati nei resoconti. La riproduzione sonora sul telefono e la prova in automobile restano distinte dai test eseguiti sull’emulatore.

Il servizio Android Auto è dichiarato nel manifest e il suo protocollo di navigazione è collaudato. Non sono ancora verificati la comparsa nel launcher di un’auto/Desktop Head Unit, i comandi al volante su hardware reale, le telefonate e la continuità su un’autoradio Bluetooth. Una build APK locale può richiedere la modalità sviluppatore/Origini sconosciute di Android Auto, come spiegato nel README e nella documentazione Google.

## Riservatezza e limiti

Nessuna richiesta Internet da parte dell’app. Nessun profilo/configurazione macOS importato. Libreria MediaStore, interfaccia iniziale in italiano, coda solo in memoria; riavvio del processo e avvio dopo reboot non riprendono automaticamente l’ascolto. Le funzionalità desktop di rete, DSP/esclusiva e visualizzazioni animate non sono incluse in questa prima versione.

## Prova fisica in background e display spento — 8 novembre 2026

Riproduzione reale avviata attraverso MusicCraft, poi circa 30 secondi sulla Home e una finestra di osservazione di tre minuti in standby, con USB collegato. In 42 campionamenti risultano sempre PLAYING, AudioTrack nello stato started, servizio foreground presente e processo invariato. Il cambio automatico fra due tracce è avvenuto durante lo standby. Nessuna impostazione di volume o risparmio energetico modificata; riproduzione messa in pausa al termine.

Si è verificata una breve riaccensione osservata: non si dichiara un tratto ininterrotto di tre minuti a display spento. Il tratto consecutivo più lungo, osservato ogni circa cinque secondi, supera due minuti; il display è risultato OFF nel controllo separato. Queste verifiche attestano stato del percorso audio e avanzamento, non sono una cattura del segnale fisico e non escludono microinterruzioni inferiori al campionamento.

Android rileva il servizio MediaBrowser di MusicCraft. Android Auto installato riconosciuto, connessione di fiducia supportata dall’accesso alle notifiche; l’app MusicCraft risulta installata direttamente tramite APK. Aperta la schermata Personalizza Avvio app; la verifica della voce MusicCraft è in corso con l’utente. Non dichiarata ancora una prova su autoradio o Desktop Head Unit.


## 0.2.1 — server musicale diretto e pairing

Build debug, APK test, tre unit test e lint debug completati (zero errori; warning di risorse, aggiornamenti e API annotati nel report Gradle). Instrumentation su emulatore isolato: identità Keystore persistente, HTTPS, autenticazione, file byte-exact e a intervalli, HEAD, Range invalidi, metodi di scrittura rifiutati, limite di tre tentativi e chiusura della finestra di abbinamento. Il server si sospende e riprende con la riproduzione Media3; la suite conserva il controllo legacy Android Auto, la riproduzione senza Activity e la sequenza automatica dei tre FLAC sintetici.

Un test desktop esplicito usa il server Android reale nell’emulatore: SRP-6a Bouncy Castle/RustCrypto interoperabile, PIN errato rifiutato, PIN corretto con approvazione, identità protetta da AES-GCM, verifica TLS successiva e campioni PCM identici agli originali. L’approvazione automatica è limitata alla fixture di instrumentation, che si rifiuta di girare su un telefono reale; non esiste un endpoint di approvazione remota nell’app ordinaria.

APK installato sul telefono collegato e confrontato byte per byte con l’artefatto finale; autorizzazione al pairing effettuata dall’utente sul telefono. Provati catalogo e lettura dal Mac, anche con Activity Android in background. Il servizio reale usa foreground connectedDevice con notifica dedicata; nessuna falsa riproduzione media per mantenerlo acceso. Deep Doze, isolamento ospiti e cambio fisico della rete non sono stati simulati sul telefono. Nessuna nuova prova con l’automobile: l’integrazione Android Auto precedente resta distinta.

Pacchetti privi di dati/configurazioni del dispositivo di collaudo; identità e credenziali sono generate da ciascuna installazione. Il pacchetto contiene le attribuzioni Apache/Bouncy Castle; chiavi di firma, link interni di fixture, codici, profili, screenshot e log restano esclusi dai sorgenti pubblicabili e dagli artefatti ordinari.


## 0.3.0 — viste indipendenti e playlist dinamiche

Build debug e APK test, undici test unitari e lint completati: zero errori lint, ventidue warning (risorse linguistiche, aggiornamenti e API riportati nel report). Otto test puri coprono criteri, anni sconosciuti, voti, gruppi parzialmente ascoltati, conteggi totali/medi e cursore senza duplicati nel ciclo o nella coda futura.

Suite nativa su emulatore Android 13 dedicato: ricerca e navigazione senza mutare la coda; Player autonomo; ricreazione dell’Activity in verticale e orizzontale durante la riproduzione; modifica della coda senza perdere posizione; rifiuto di una sostituzione priva di file validi; playlist salvate e voti; almeno quattro avanzamenti automatici del mix; assenza di ripetizioni adiacenti; coda limitata; pausa senza falsi ascolti; arresto delle aggiunte con coda conservata e successiva fine naturale. Restano nella suite HTTPS/autenticazione/pairing/sola lettura e browsing Media3/legacy Android Auto.

Una prova distinta dopo force-stop conferma recupero di coda, posizione, playlist, regole e voti, senza partenza automatica dell’audio. Due esecuzioni complete consecutive sono passate dopo iniziali fallimenti temporali nel controllo preesistente della sequenza in background; non è stata dimostrata una causa applicativa di quei primi timeout. Le verifiche del percorso Media3 non sono una cattura del segnale acustico e non escludono microinterruzioni.

Ispezionate schermate reali di ricerca, playlist e Player, incluse disposizione e comandi in orizzontale, su emulatore. Non eseguita una nuova prova su autoradio o Desktop Head Unit per questa versione. Statistiche e voti sono locali; nessuna importazione da Lyrion. Screenshot, log e fixture d’esecuzione rimangono negli output ignorati, fuori dal pacchetto ordinario.


La 0.3.0 (versionCode 4) è stata installata come aggiornamento sul telefono Android 16 collegato. Hash SHA-256 del base.apk installato identico all’artefatto finale; avvio a freddo riuscito e libreria reale disponibile nella vista Cerca. Nessun permesso concesso via script né file musicali modificato. I tocchi remoti non hanno cambiato in modo affidabile la vista sul dispositivo: la prova completa delle nuove viste e della rotazione resta quella sull’emulatore, distinta dalla verifica fisica di installazione e avvio.
