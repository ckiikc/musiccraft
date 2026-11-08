# Roadmap Android

## 0.1.0 — prima versione di prova

Libreria MediaStore; brani/album/artisti/ricerca; riproduzione in background con servizio Media3, notifica e controlli di sistema; servizio di navigazione per Android Auto; APK debug firmato. Verifiche e limiti effettivi sono in VALIDAZIONE.md.

## 0.2.1 — condivisione diretta

Server HTTPS in sola lettura, scoperta locale e pairing con quattro cifre e approvazione sul telefono. Il server si sospende durante l’ascolto locale. Credenziali generate per installazione; nessuna configurazione personale distribuita.

## 0.3.0 — navigazione e playlist

Viste Cerca, Playlist e Player distinte; Player autonomo adattato a verticale/orizzontale; coda esplicita modificabile, playlist salvate e regole dinamiche con voti e cronologia locale. Ripristino in pausa di coda e posizione; nessuna riproduzione avviata automaticamente. Mix dinamici esposti al browser Android Auto. Vedere README e VALIDAZIONE per semantica e limiti.

## Passaggi successivi

- Prova sul telefono reale con permesso audio e libreria personale; controllo della riproduzione con schermo spento, cuffie/Bluetooth e interruzioni telefoniche.
- Collaudo visibile nel launcher Android Auto, su auto o Desktop Head Unit.
- Cartelle selezionabili, gestione dei supporti rimovibili e ulteriore prova delle interruzioni hardware.
- Traduzioni e visualizzazioni MusicCraft; sincronizzazione e sorgenti di rete come attività separate.
