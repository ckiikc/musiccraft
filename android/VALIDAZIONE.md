# Verification 0.1.0 — 8 November 2026

This log records the checks performed for each version and date. Later entries supersede earlier implementation limitations; translating this document does not imply that the tests were rerun.

## Build and package

Debug build completed with compile/target SDK 36, Media3 1.11.1 and Kotlin 2.2.20. APK signed and verified with apksigner (v2 scheme); application `it.ckiikc.musiccraft`, versionCode 1, version 0.1.0, minSdk 26. The APK contains licenses, but no test music, personal data or private keys.

Two unit tests passed: title/artist/album search and pagination with invalid inputs and numeric limits. Android Lint: 0 errors, 11 warnings, mainly about language resources for the initial Italian version, wrapper updates, the icon and intentional public-service/application-context configuration. These warnings do not provide additional hardware compatibility evidence.

## Dedicated Android 13/API 33 emulator

Instrumentation completed with three synthetic FLAC files and real tags. The runner refuses physical phones and cleans up only its own fixtures, except for the explicit emulator visual-inspection option.

Verified:

- Library reading and actual rows on the phone screen.
- Media3 and legacy Android MediaBrowser connections, root and Tracks/Albums/Artists categories, browsing, pagination and search.
- Library identifier resolution through the audio service.
- Playback startup, previous/next, seeking and pause.
- Continued playback after `Activity.finish`, including a track change.
- Automatic Gamma → Alpha → Beta sequence and stopping at the end without repeat.
- A real list-row tap and session state `PLAYING`; playback remained active after returning Home.

Actual library/player screenshots inspected; synthetic captures and logs remain in ignored workspace outputs.

## Phone and Android Auto

An Android 16 phone was detected through ADB, with Android Auto installed. No previous MusicCraft Android installation was found. USB installation of 0.1.0 succeeded after user confirmation. Installed version and APK SHA-256 were verified against the delivered package. Cold startup completed in 721 ms, without detected process crashes. No permissions were granted through scripts and no phone music files were changed. The user subsequently granted audio permission on the phone: the real library populated and rows appeared without player error messages. Personal titles/artists were not copied into reports. Audible phone playback and car testing remain separate from the emulator checks.

The Android Auto service is declared in the manifest and its browsing protocol was tested. Appearance in a car/Desktop Head Unit launcher, steering-wheel controls on real hardware, phone calls and Bluetooth head-unit continuity were not yet verified. A local APK may require Android Auto developer mode/Unknown sources, as described in the README and Google documentation.

## Privacy and limitations

At this version the app made no Internet requests. No macOS profile/configuration was imported. MediaStore library, initial Italian UI and an in-memory queue; process restart or reboot did not automatically resume listening. Desktop network features, DSP/exclusive output and animated visualizations were not included in the first version.

## Physical background and screen-off test — 8 November 2026

Actual playback started through MusicCraft, followed by approximately 30 seconds on Home and a three-minute standby observation window with USB connected. All 42 samples showed PLAYING, AudioTrack started, a foreground service and the same process. An automatic transition between two tracks occurred during standby. Volume and power-saving settings were unchanged; playback was paused at the end.

A brief screen wake was observed: this is not a claim of three uninterrupted screen-off minutes. The longest consecutive interval, sampled about every five seconds, exceeded two minutes; a separate check reported the display OFF. These checks establish audio-path state and progress, not a physical-signal recording, and do not exclude gaps shorter than the sampling interval.

Android detected MusicCraft's MediaBrowser service. Android Auto was identified, with trusted connections supported through notification access; MusicCraft was installed directly by APK. Customize launcher was opened and its MusicCraft entry was being checked with the user. No head-unit or Desktop Head Unit test was claimed at that point.

## 0.2.1 — direct music server and pairing

Debug build, test APK, three unit tests and debug lint completed: zero errors; resource, update and API warnings recorded in the Gradle report. Isolated-emulator instrumentation checked persistent Keystore identity, HTTPS, authentication, byte-exact and ranged file access, HEAD, invalid ranges, rejected write methods, the three-attempt limit and pairing-window closure. The server suspended/resumed with Media3 playback; the suite retained legacy Android Auto checks, playback without an Activity and the three-FLAC automatic sequence.

An explicit desktop test used the real Android server inside the emulator: Bouncy Castle/RustCrypto SRP-6a interoperability, incorrect PIN rejection, correct PIN plus approval, AES-GCM-protected identity, subsequent TLS verification and PCM samples identical to the originals. Automatic approval is confined to the instrumentation fixture, which refuses physical phones; the ordinary app has no remote-approval endpoint.

APK installed on the connected phone and compared byte-for-byte with the final artifact; the user approved pairing on the phone. Catalog access and Mac reads were tested, including with the Android Activity in the background. The real service uses foreground connectedDevice with a dedicated notification, without fake media playback to keep it alive. Deep Doze, guest isolation and physical network changes were not simulated on the phone. No new car test was performed; earlier Android Auto integration remains a separate result.

Packages contain no test-device data/configuration; each installation generates its identity and credentials. Apache/Bouncy Castle attributions are included. Signing keys, internal fixture links, codes, profiles, screenshots and logs remain excluded from publishable source and ordinary artifacts.

## 0.3.0 — independent views and dynamic playlists

Debug and test APK builds, eleven unit tests and lint completed: zero lint errors, twenty-two warnings, with resource, update and API details in the report. Eight pure tests cover criteria, unknown years, ratings, partly played groups, total/average counts and a cursor without duplicates in the cycle or upcoming queue.

Native tests on a dedicated Android 13 emulator checked browsing/search without queue mutation; standalone Player; portrait/landscape Activity recreation during playback; queue edits without losing position; rejection of replacement with no valid files; saved playlists and ratings; at least four automatic mix transitions; no adjacent repeats; a bounded queue; pause without false play counts; stopping additions while retaining the queue and then reaching its natural end. HTTPS/authentication/pairing/read-only access and Media3/legacy Android Auto browsing remain in the suite.

A separate force-stop test confirmed recovery of queue, position, playlists, rules and ratings without automatic audio startup. Two consecutive complete runs passed after initial timing failures in the existing background-sequence check; an app cause for those first timeouts was not established. Media3-path checks are not acoustic recordings and do not exclude microinterruptions.

Actual emulator Search, Playlists and Player screens were inspected, including landscape layout and controls. No new head-unit/Desktop Head Unit test was performed for this version. Statistics and ratings are local; there is no Lyrion import. Runtime screenshots, logs and fixtures remain in ignored outputs outside the ordinary package.

Version 0.3.0 (versionCode 4) was subsequently installed as an update on the connected Android 16 phone. Installed base.apk SHA-256 matched the final artifact; cold startup succeeded and the real library appeared in Search. No permissions were granted by script or music files changed. Remote taps did not reliably change the device's view: full new-view and rotation verification remains the emulator result, separate from physical installation/startup checks.

## 0.3.1 — visible sharing

Debug build, eleven unit tests and lint completed: zero errors, twenty-two warnings. Native manual checks on an isolated Android 13 emulator: Share directly in the header, explicit Off state, real service activation, panel retained through startup and updated with Pair computer when ready, without reopening the menu. Portrait and landscape Player screenshots with the new status were inspected. Panel text changes only when status changes; updates stop when the panel closes or the Activity goes into the background.

The ordinary package is signed for local testing and contains no audio fixtures, keys or phone configuration. This version was prepared as a local APK, not published on GitHub. No new audio or Android Auto test is claimed for this UI change.

### Phone update and reconnection — 9 November 2026

Version 0.3.1 (versionCode 5) installed as an update with unchanged signing certificate and UID. Identity-preference fingerprints matched before/after installation; no data was cleared. The new Share button and explicit Off state were verified on-device. Remote taps had no effect; the user performed Share → Enable and confirmed activation.

MusicCraft macOS 0.10.50 retained an old pairing panel opened while the phone was off and an outdated Bonjour name. After closing that panel and restarting the idle desktop app normally, automatic discovery found the phone's new network name and updated the source using the original credentials. Device identifier, TLS pin and saved account were unchanged; no new code or pairing approval was required. The Android catalog showed 7,006 tracks and a completed check with zero errors. This verifies the existing authenticated connection, not a new acoustic test. Test screenshots, profiles and logs remain in ignored outputs.

## 0.3.2 — Android Auto playlists and per-car queues

Debug/test APK builds completed; fifteen unit tests passed, lint reported zero errors and twenty-eight warnings. New pure checks cover unique car selection, unassociated devices, ambiguity, optional consent, the Android Auto requirement and queue links invalidated by profile/content changes. Recognition and profile data are generic per installation: no personal name, address or credential is preconfigured.

Instrumentation on a new dedicated Android 13 emulator with three synthetic FLAC files checked playlist/profile names in Media3 metadata and the legacy Android queue/session; starting a dynamic mix through playFromMediaId without touching the Activity; Now playing/Playlists/Library/Cars and queues categories; hosts with two main tabs; reading/selecting a queued track without losing the mix; saved playlists started through browsing; Phone → Car A → Car B changes with separate queues, positions, modes and generators, paused restoration and protection against removing the active profile. Independent search, portrait/landscape Player, transitions/background, persistence, HTTPS and read-only sharing remain in the suite. The first run stopped at the existing audio-restart check: the test now waits for READY and actual progress of the new sequence rather than the controller's provisional isPlaying state alone. Subsequent complete runs passed. This is not an acoustic recording.

A separate force-stop test recovered queue, position, playlists, rules, ratings and saved profiles without automatic playback. Migration from synthetic old-format JSON, without profiles or new queue information, retained existing data in the Phone profile; Bluetooth recognition initially remained disabled. Actual emulator screenshots verified the consent panel and Player/profile views. The generic unidentified-connection profile is reserved and cannot be associated with a specific car.

Bluetooth recognition uses official Android Auto connection events and already paired audio devices. Only legacy BLUETOOTH and optional BLUETOOTH_CONNECT are required, without scanning or location. Real hardware events and queue rendering on a head unit/Desktop Head Unit were not tested in that session. Protocol/selection-policy checks do not certify every phone/car combination. Local APK signed, not yet installed on the personal phone or published on GitHub at that stage; fixtures, profiles and logs remain in ignored outputs.

### Phone update to 0.3.2 — 9 November 2026

Ordinary USB update succeeded without uninstalling or clearing data. Version 0.3.2/versionCode 6, unchanged certificate/UID, installed base.apk hash identical to the tested artifact. Private backup of the previous APK and listening state retained in ignored outputs.

Numeric MediaStore inventory matched before/after. Playlists, dynamic rules, ratings, queue identifiers and position were retained during migration into Phone; the generic car profile had a separate empty queue. Android sharing-preference fingerprints and the phone identity/TLS pin stored on the Mac were unchanged, without reading the Keychain secret or starting new pairing. No new playback test or Mac HTTPS connection was claimed for this installation.

The phone locked again before visual profile inspection. The user was asked to open Player → Cars and queues. UI profile switching and Bluetooth association setup were still pending at that point; no physical-device permissions were granted by script. Full queue testing remained the emulator result, separate from car hardware testing.

### Car profile and Bluetooth setup completed — 9 November 2026

Cars and queues was verified in the phone UI. The user created a car profile, approved Nearby devices in Android's prompt, selected the already paired head unit and enabled recognition. BLUETOOTH_CONNECT, the saved association fingerprint and the automatic option were verified on-device; no permission was granted by script and no personal identifier was included in source.

Manual switching to the car profile showed zero tracks in the actual UI; the Phone queue remained fully preserved in the state file. The user confirmed returning to the original queue on screen; USB had been disconnected before the final capture, so that last transition is supported by user confirmation. The user was not in the car: automatic recognition on a physical Android Auto/Bluetooth connection and behavior on a particular head unit were not claimed. Setup completed and separate queues verified on the phone, with in-car hardware testing still separate.

## 0.3.3 — Bluetooth crash and sharing continuity

On the Android 16 phone running 0.3.2, PlaybackService shutdown caused a process crash: ConcurrentModificationException in CarRouteMonitor.close. Synchronous proxy disconnection modified the map during cleanup. Logs also showed a case with SharingService active, terminated with the process when backgrounded. The server was not merely suspended by audio.

Two regression tests reproduce synchronous disconnect callbacks and reentrant close: both fail with the old iteration and pass after resources are detached first. Monitor cleanup is idempotent and Bluetooth callbacks are serialized on the main thread; late callbacks release their proxy without accessing the closed player.

Ordinary/test builds, seventeen unit tests and lint completed: zero errors, twenty-eight warnings. General instrumentation passed on the second run; the first failed the background-audio check already intermittent in the previous version: IDLE after Activity closure, without a new crash. Neither that check nor the audio engine was modified; the counter alone does not establish acoustic stability.

A separate Android 13 emulator test kept the real foreground HTTPS authenticated catalog available through three PlaybackService creation/destruction cycles, with Activity closed and no local audio. An initial attempt was interrupted by the operator through force-stop and does not count as an app result; the subsequent isolated run passed. Recovery after force-stop retained queues, position, playlists, criteria, ratings and profiles without automatic audio.

Version 0.3.3 installed as a phone update: unchanged certificate/UID, installed APK identical to the tested artifact, private state byte-identical and matching music inventory before/after. Identity-preference fingerprint unchanged; no deletion, new pairing or permissions granted by script. Logs, backups and personal details remain in ignored outputs.

Physical verification after manual activation through Share: SharingService remained foreground, the same process survived backgrounding and PlaybackService was confirmed terminated; no new crash in the process buffer. The Mac received Bonjour and completed the Android source check with zero errors using the existing pairing. Preexisting desktop listening was left active; no acoustic recording or new car test was performed.

## 0.3.4 — second crash during HTTPS suspension

A subsequent phone check found 0.3.3 terminating with NetworkOnMainThreadException in SharingServer.suspendTransfers: closing an active SSLSocket can send close_notify. The SharingService ticker performed this on the main thread when local audio started. The previous Bluetooth crash did not recur; this was a second, distinct path.

Reproduced on an Android 13 emulator: TLS client with completed handshake and incomplete HTTP request, suspendTransfers called on main. The test fails with NetworkOnMainThreadException on the previous version and passes on 0.3.4. Main-thread server shutdown is also tested with an active client. Cleanup now uses a dedicated worker, detaches resources before callbacks, batches cleanup and drains already submitted resources during shutdown. Listeners/clients accepted after shutdown do not become ownerless; no TLS operations on main and no authentication, permission or sharing-policy changes.

Ordinary/test builds, nineteen unit tests and lint completed. Pure tests with a blocked simulated socket and a close error confirm a nonblocking caller and release of subsequent resources during shutdown. General instrumentation passed on the first run for this version. Separate real-foreground-service test: three cycles with open TLS, local-audio suspension, catalog rejection during audio, authenticated resumption and audio-service shutdown without losing the server.

Version 0.3.4 installed on the phone as an update with certificate/UID retained, installed APK identical to the artifact, listening state byte-identical and matching music inventory. Sharing identity unchanged, without new pairing or permissions granted by script.

Physical 0.3.4 test: Mac TLS connection checked against the existing paired pin, request left incomplete and existing-queue playback commanded through MusicCraft's media session. The connection closed in 1.317 seconds when local audio activated; after Pause the server responded again. Same PID, SharingService still active, queue identifiers retained after backgrounding. No acoustic recording. Two initial verifier attempts failed to recognize status promptly: Android 16 prints PLAYING(3), and the interactive monitor buffers notifications. The final check uses status returned directly by dumpsys; every test exit path returns the phone to pause.

## 0.4.0 — Continuity and persistent artwork — 10 November 2026

Debug/test APKs, 23 unit tests and lint completed: zero errors, 45 warnings. Actual native screens inspected on an isolated Android 13 emulator: Playlists home, Dynamic/Saved collections, Search with images and track menu, Player, queue, car profiles and Artwork panel. Compared actual screens against design proposal 3, Continuity; landscape Player also checked at 130% font size, portrait views at 320×640 dp and 130% fonts. Transport remained visible, secondary content scrollable and navigation centered/selected. Fixed the new options button's focus behavior, which blocked the row action when its text was tapped; verified through actual emulator taps. Profile changes updated the screen immediately without automatic playback.

Full suite covered search without queue replacement, rotation during playback, queue edits, playlists/ratings, mix refill/stop, per-car queues/positions, automatic sequence/background, read-only HTTPS and suspension/resumption. One run stopped at an existing test that waited only 300 ms after Stop before querying sharing: Android still reported audio active. The wait now checks actual gate release within a deadline; HTTP 200 remains mandatory. The repeated full run passed, along with separate force-stop recovery of queue/position, playlists, rules, ratings and profiles without autoplay. The engine and sharing gate were unchanged. These tests are not acoustic recordings.

Artwork checks covered SQLite reopening and permanent files, deduplication of identical images, album aliases, independent loose-track associations, distinct embedded images within the same album, malformed images, byte/storage limits and a read-only provider without traversal. Controlled transport checked retrieval counts: one acquisition per album, no second request after reopening, remembered negative results. A separate real-network test with public Kind of Blue metadata retrieved MusicBrainz/Cover Art Archive artwork, validated the JPEG and reopened it without another request. This does not establish that every metadata record/track finds an image: uncertain matches remain without artwork. No music file or album folder was written.

Emulator update 0.3.4 → 0.4.0 retained the UID and all preexisting state files byte-for-byte before first launch. ListeningStore format, queues, profiles, identity, certificate and pairing were unchanged. Final signed APK hash matched the build actually installed in the emulator; synthetic audio/artwork are confined to the test package. Ordinary package assets contain only icon licenses, with no device data, configuration, database or credentials. The personal phone was not changed or updated during that session.

Android Auto: Media3/legacy clients verified Now playing/Mixes/Playlists/Library, fewer-tab fallback, queue/profile names, mix startup and queue selection. Rendering on a real Android Auto/head-unit display was not verified: the emulator stub's signature was incompatible with the available Google distribution, and Desktop Head Unit was not connected to the personal phone. The proposal's car panel remains illustrative. Bluetooth recognition and in-car audio hardware require separate testing. Visual evidence and logs remain in ignored outputs.

### Phone update to 0.4.0 — 10 November 2026

Following a separate explicit user request, 0.4.0/versionCode 9 was installed by USB as an ordinary update. Signing SHA-256 matched the installed 0.3.4 certificate, UID was retained and the phone APK hash matched the tested artifact. Previous APK/state backed up privately in ignored outputs, without reading or exporting Keystore keys.

MediaStore inventory remained identical: 7,031 tracks. Before first launch, all listening-state/preference files were byte-identical; after launch, statistics, eight dynamic rules, saved playlists, profiles, Bluetooth configuration and every queue matched semantically. Sharing identity was retained. No uninstall, data deletion, new permission grant or physical-device instrumentation. App 0.4.0 opened as the current Activity with its process active.

Sharing had been active before the update and stopped when the app was replaced. The phone ignored remote taps; Android rejected directly starting the private component through the management command. The user was asked to reactivate Share → Enable without new pairing. The user confirmed activation; SharingService was confirmed foreground. MusicCraft on the Mac had been closed and was launched while preserving user focus. The saved Android source reported Check completed, Read: 1, Errors: 0, using the existing pairing. The configuration panel proposing a new pairing was closed without entering codes or authorizing another computer. The initial mini-player view was restored without starting audio. This verifies an authenticated connection, not a new acoustic test.

## 0.4.1 — tag interoperability and exact play-count rules (2026-11-13)

Build, 25 JVM tests and Android Lint passed. Emulator API 33 instrumentation verifies the shared Rust JNI FLAC/MP3 engine, byte-identical audio payload and preserved custom comment, read-only inspection, approved tag writes, interrupted metadata-write recovery, rejection of unauthenticated/invalid requests and manual-vote precedence. Rating/count rules round-trip through the JSON codec and legacy rules retain unbounded count defaults. Inclusive count bounds combined with votes and unknown-versus-explicit-zero ratings have focused tests.

The real macOS HTTPS client completed a synthetic-file rating change after both the app approval dialog and the Android MediaStore write-permission dialog were operated in the emulator. The repeated identical request needed no further write/approval. The real criteria editor was visually inspected, including minimum/maximum plays. No personal phone, physical car or personal audio file was used. The initial safe write scope and recovery limits are documented in the shared engine README. Installation on physical hardware remains unverified; this APK was installed only in the dedicated emulator.

## 0.4.3 — artwork URI regression

The 0.4.2 APK reproduced FileNotFoundException at ArtworkProvider's ID validator with two generated embedded JPEGs in synthetic FLACs. The corrected build passed 26 JVM tests, Lint and native API33 emulator artwork checks: persistence/reopen, deduplication, 0.4.2 positive and negative association migration without lookup, per-track images, album artwork, invalid image/bounds, traversal and mutation rejection. The framework MediaBrowser/MediaController used by Android Auto returned readable cover URIs for songs, album entries and the playing item. A separate UID was denied initially, read a decoded 48x48 JPEG after the same prefix read grant used by controllers, could not write, read identical bytes afterward and lost access after revocation. Only the emulator ran instrumentation. This does not assert an actual car/DHU screenshot or that every song has an embedded/local/online cover.
