# MusicCraft for Android — 0.4.1

## Rating tags and paired desktop edits — 0.4.1

The menu **Voti nei tag** chooses the numeric VOTE/RATING scale. Indexing is read-only and caches raw values separately from manual ratings; existing ratings and explicit zero remain authoritative. Paired desktop computers can request a rating change, which names the song and requires approval in MusicCraft plus Android media-write permission. No arbitrary file-write/upload endpoint is exposed. The initial safe writer handles FLAC and MP3 metadata prefixes, journals interrupted edits and preserves audio bytes; unsupported layouts remain unchanged. See [the shared engine documentation](../crates/musiccraft-tags/README.md). This build contains arm64-v8a and x86_64 native libraries.


The first native Android version of MusicCraft. This is a separate project from the macOS app and does not change its profile or audio engine. It is not an official ArtCraft release.

## Features

- A local music library indexed by Android MediaStore, including indexed shared storage and removable cards: tracks, albums, artists and search.
- Media3/ExoPlayer playback through a single `MediaLibraryService`, shared by the phone, notification, system controls and Android Auto.
- Play/pause, previous/next, seeking and automatic track transitions. Audio continues when the app screen closes; the player handles audio focus and headphone disconnection.
- Artwork, title, artist and album; manual library refresh. Online lookup for missing artwork is optional and disabled by default; optional local-network sharing uses HTTPS.
- Catalog browsing and search through both Media3 clients and the `MediaBrowserService` protocol used by Android Auto. `MediaSession` checks connections: the app itself, the notification controller and controllers trusted by the system are allowed.
- Voice search through the media session and `MEDIA_PLAY_FROM_SEARCH`.

The initial UI is in Italian; translated labels below describe the functions, while the app may show their Italian names. Vinyl/VU visualizations, FTP/NAS and folder import through SAF are outside this version. Actual format support also depends on the device's decoders. The library shows shared audio indexed by Android, not other apps' private files.

## Continuity — 0.4.0

The **Playlists** home screen offers Resume listening, two useful mixes and access to all playlists, separated into Dynamic and Saved. Search does not change the queue; the standalone Player shows artwork, transport and the next two tracks. The landscape layout keeps transport controls at the top; secondary content scrolls even with larger fonts. Phone/car context and the queue name remain visible. Cars and queues has a dedicated screen. Sharing remains directly accessible.

Android Auto exposes four main destinations: **Now playing, Mixes, Playlists, Library**. Hosts allowing fewer tabs retain access through intermediate menus. The car display uses system templates: the design proposal is illustrative. Creating/editing mixes and configuring Bluetooth remain phone operations; existing mixes can be started from the car.

Updates retain the applicationId, signing identity, listening-state format, playlists, queues, Bluetooth profiles and HTTPS/pairing identity. Install as an update to retain data; do not uninstall or clear app data.

## Persistent artwork

Under **⋮ → Artwork** (`Copertine`), enable missing-artwork lookup and view storage usage. Initial lookup priority is: artwork embedded in the individual file, local MediaStore artwork for identifiable albums, previously stored artwork, then optional online lookup. Existing positive results are reused immediately; a change to the file or metadata creates a new association.

The store uses a private SQLite index and JPEG files under `files/artwork`, outside Android's disposable cache. Each music file has its own identity; lookups for the same album and identical image content are shared without duplicating artwork. Tracks with unknown albums use artist/title lookup rather than folder grouping. Distinct embedded images remain distinct even when tracks have the same album. Images are not written into music folders: no write permission is needed, and a folder containing unrelated tracks is never treated as one album. Uninstalling or clearing all app data also removes this store.

When lookup is enabled, only the required metadata is sent to MusicBrainz: artist/album, or artist/title. Paths, filenames and pairing information are not sent. Images come from Cover Art Archive. Loading is on demand for tracks displayed or requested by a media client, with a bounded worker; it does not start an online scan of the entire library. Uncertain matches are left without artwork. Availability is not guaranteed for every album or incomplete metadata.

Images are validated, resized and deduplicated. The store is limited to 256 MiB, without automatically deleting retained artwork. Missing results are remembered for seven days and temporary errors for at least fifteen minutes; service limits and Retry-After impose a pause that survives restart. Requests to the MusicBrainz API are spaced at least 1.1 seconds apart. The provider exposes only local, read-only artwork URIs; access is granted to trusted media controllers without exposing the app's private files.

References: [Android Auto artwork](https://developer.android.com/training/cars/media/create-media-browser/media-artwork), [Cover Art Archive API](https://musicbrainz.org/doc/Cover_Art_Archive/API), [MusicBrainz rate limits](https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting).

## Search, Playlists and Player

The bottom navigation separates three views. **Search** (`Cerca`) explores and filters the library without changing the queue: each track offers Play now, Play next, Add to queue, Add to playlist and Rate. Replacing an existing queue requires confirmation. **Playlists** contains saved lists and dynamic mixes; **Player** is a standalone view with artwork, seeking and transport controls. In landscape, artwork and controls are arranged side by side; the service maintains audio while the screen is recreated. The mini player in other views opens the full Player.

Open **Queue** from the Player to move, remove, play or save tracks. Queue and position are saved periodically and restored paused after a process restart; audio never starts automatically. Missing files are excluded from restoration.

### Dynamic playlists

Eight initial templates and a criteria editor support tracks, albums or artists; genre; year or range/decade; ratings from 0 to 5; text; played, unplayed and partly played groups. Ordering supports random, total/average play count, total/average rating, number of rated tracks and recent listening. A preview is available before starting. Metadata and rating filters select tracks before grouping; albums/artists are evaluated on that selected subset. Unknown years are excluded when year limits are set.

A mix adds a bounded number of tracks ahead and retains at most twenty previous queue entries. It avoids duplicates within a cycle and among upcoming tracks; it can restart after a cycle ends. Unplayed-only mixes eventually run out. **Stop adding automatically** stops the generator while preserving the existing queue. Criteria changes apply the next time the mix starts.

A play counts after 30 seconds of actual listening or half the track, whichever is shorter, with a one-second minimum; pauses and seeking do not add listening time. Ratings, statistics, rules and playlists are private to the installation. This behavior is inspired by Lyrion dynamic playlists, without an LMS server connection, imported LMS statistics or all of its presets. Mixes are also available through Android Auto's MediaBrowser navigation; creating/editing them requires the phone UI.

## Android Auto: playlists and car queues — 0.3.2

**Now playing** shows the playlist/profile and queued tracks. Dynamic mixes and saved playlists can be started directly from the car. Each profile retains a separate session, including position and mix state. From the phone Player, open **Cars and queues** (`Auto e code`), create a profile, associate the head unit's Bluetooth device and enable optional recognition. Manual selection remains available on the car display. Connecting never starts music by itself; previous listening data is retained in the Phone queue. See [CAR_QUEUES.md](CAR_QUEUES.md) for the procedure and limitations.

## Visible sharing — 0.3.1

The **Share** (`Condividi`) button and sharing status remain visible in all three views. Compact status distinguishes Off, Active and Suspended; the panel explains why the computer cannot find the phone when the service is off. It stays open after **Enable** (`Attiva`) and shows startup, availability, suspension or waiting for Wi-Fi. Once ready it directly offers **Pair computer** (`Abbina computer`); previously paired computers reconnect without another code. Reopening the overflow menu is unnecessary. Enabling sharing remains an explicit user choice.

## Direct computer sharing

Available to every installation, without preconfigured IP addresses or passwords: press **Share → Enable**, choose **Pair computer**, then enter the four digits in MusicCraft desktop 0.10.49 or later, which discovers the phone automatically. Confirm **Authorize** (`Autorizza`) on the phone. Access is read-only and suspended during phone audio playback, with a notification and pairing revocation. No separate FTP app is needed. The procedure, protocol, limits and security are documented in [SHARING.md](../docs/SHARING.md).

## Requirements and build

Android 8.0/API 26 or later. The project pins compile/target SDK 36, JDK 21, Gradle 8.14.3, AGP 8.10.1, Kotlin 2.2.20 and Media3 1.11.1.

Set `ANDROID_HOME` or create an ignored `local.properties` containing `sdk.dir=...`, then run:

```sh
./gradlew :app:assembleDebug :app:testDebugUnitTest :app:lintDebug
```

APK: `app/build/outputs/apk/debug/app-debug.apk`. The debug APK is signed for local testing; it is not a Google Play release. Private signing keys and local paths must not be published. Updating an installation requires the same certificate and a compatible versionCode.

## Phone test

1. Install the APK and open MusicCraft.
2. Press **Allow music access** (`Consenti accesso alla musica`) and grant audio/music permission in Android's prompt. Notification permission is separate; denying it does not replace the library permission.
3. Open a track, press Home or close the screen, then check the notification and automatic transition to the next track.
4. If the library is empty, check that files are in shared storage and indexed by Android, then choose **⋮ → Refresh library** (`Aggiorna libreria`).

Do not grant phone permissions through scripts: the user must approve prompts on the device. Automated tests that create/remove fixtures refuse to run on physical phones.

## Android Auto

The manifest declares MusicCraft as an Android Auto media service. First grant audio permission in the phone app. A sideloaded APK may require **Unknown sources** in Android Auto's developer settings; this is separate from Android's APK installation settings. Google Play and internal channels are available for ordinary distribution and further testing.

Media-protocol testing alone does not establish that the app appears in a car launcher. A real head unit or Desktop Head Unit display and controls require a separate test.

Official references:

- https://developer.android.com/media/implement/surfaces/cars
- https://developer.android.com/training/cars/testing
- https://developer.android.com/media/media3/session/background-playback

## Architecture

`Catalog.kt` reads MediaStore on a worker, retains a snapshot and serves stable identifiers with bounded pagination. `PlaybackService.kt` owns the single player and resolves request IDs against the library: clients cannot supply arbitrary URIs. `PlaybackCoordinator.kt` manages the queue, listening statistics and dynamic generator in that same service. `CarRouteMonitor.kt` observes Android Auto connections and associated Bluetooth devices without scanning. `ListeningStore.kt` writes snapshots using AtomicFile on a worker; unreadable files are retained and reported. `SmartRules.kt` implements pure, testable selection. `MainActivity.kt` uses a MediaBrowser/MediaController and never opens a second player. Releasing a screen's controller does not release the playing service.

## Verification

See [VALIDAZIONE.md](VALIDAZIONE.md) for actual results. Instrumentation tests use three synthetic FLAC files and a separate emulator. Build `:app:assembleDebugAndroidTest`, install the app and test APK on the emulator, grant audio/notification permissions there, then run:

```sh
adb -s EMULATOR_SERIAL shell am instrument -w -r \
  it.ckiikc.musiccraft.test/it.ckiikc.musiccraft.MusicCraftInstrumentation
```

Do not install the test APK on a personal phone. The runner resets only MusicCraft state on the dedicated emulator. Fixtures are isolated under `Music/MusicCraft QA` and cleaned up before the test ends.

### Numeric play-count criteria — 0.4.1

Dynamic playlist rules also support inclusive minimum and maximum play counts, combined with the rating range and other criteria. Blank minimum means zero; blank maximum means no upper bound; maximum zero selects never-played tracks. For example: minimum rating 4, maximum plays 2. Counts are qualified plays recorded on this device, not a combined Mac/phone history. Existing saved rules without count bounds retain their earlier behavior. Android Auto can start these saved mixes; editing criteria remains a phone operation.
