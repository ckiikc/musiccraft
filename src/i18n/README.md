# Localizzazione MusicCraft

MusicCraft supporta le 23 lingue elencate nella documentazione pubblica di
PdfCraft. L’elenco comprende inglese, giapponese, cinese semplificato e
tradizionale, ceco, portoghese brasiliano, tedesco,
spagnolo, francese, russo, kazako, bulgaro, telugu, ungherese, ucraino,
italiano, arabo, polacco, sloveno, greco, turco, lituano e vietnamita.

Le chiavi inglesi sono anche il fallback. I 22 cataloghi TSV hanno 321 voci
ciascuno, con due colonne per testo sorgente e traduzione.
`tr!` risolve testo statico e `format` sostituisce placeholder nominali in un
solo passaggio. I valori inseriti sono opachi: nomi di file, dispositivi,
metadati, URL e credenziali non vengono tradotti o reinterpretati. Gli errori
previsti del motore sono tradotti al confine UI; diagnostiche sconosciute del
sistema restano originali.

La modalità automatica legge prima LC_ALL, LC_MESSAGES e LANG, poi le
preferenze di macOS. La scelta manuale si salva nel profilo e si applica
subito, mantenendo l’identità delle finestre. I font
macOS sono letti dal sistema, mai ridistribuiti: Arial per i testi latini e
fallback di sistema per CJK, telugu e arabo. Per l’arabo il primo font latino
di egui separa le parole arabe dagli spazi e dai numeri, evitando che il suo
shaper inverta i numeri. Il riordino Unicode riguarda soltanto il testo UI,
non i tag musicali. Il layout dei controlli non viene specchiato interamente.

La selezione lingua carica temporaneamente i font necessari a mostrare tutti
gli autonimi, poi conserva soltanto quelli della lingua scelta. Le traduzioni
sono state preparate con assistenza AI e richiedono una successiva revisione
editoriale da madrelingua. Le prove tecniche verificano il manuale in tutte
le lingue e includono controlli visivi sui diversi sistemi di scrittura.

Prima di aggiungere una chiave, aggiornare tutti i TSV e i controlli di copertura.
La suite verifica copertura, duplicati, chiavi UI, placeholder, plurali, valori
opachi e la ricerca nelle nove sezioni del manuale per ogni lingua. La ricerca
in arabo usa il testo nell’ordine di lettura. I conteggi in vietnamita usano
sempre la stessa forma. La provenienza e l’impronta SHA-256 del nuovo
catalogo sono in [ATTRIBUTION.toml](../../ATTRIBUTION.toml). Non usare questo modulo per normalizzare
o riscrivere la libreria.

Il cambio lingua aggiorna la biografia seguendo i collegamenti tra le voci
Wikipedia dello stesso artista, con l’inglese come ripiego. Il cinese usa
il wiki zh con la variante hans o hant, il portoghese brasiliano usa pt e il
vietnamita usa vi. La cache delle biografie è separata per lingua. I testi
delle canzoni conservano la lingua originale.
