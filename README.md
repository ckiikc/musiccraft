# MusicCraft

A music player for local collections, mounted NAS libraries and music shared by a paired Android phone. A faceted desktop library, independent playlists and playback queues, and a mobile player with Android Auto keep browsing separate from listening.

**Independent project · experimental releases · MIT source license.** MusicCraft is not an official ArtCraft application. [Community-app consideration](https://github.com/storytold/craft-launcher/issues/1247) does not imply affiliation or acceptance.

## Downloads and release lines

| Platform | Release | Status |
| --- | --- | --- |
| Apple silicon macOS | [0.10.63](https://github.com/ckiikc/musiccraft/releases/tag/v0.10.63) | Current Mac/Android feature line; ad-hoc signed, not notarized; optional LMS runtime excluded |
| Android | [0.4.2 test APK](https://github.com/ckiikc/musiccraft/releases/tag/android-v0.4.2) | Android 8+ for music, Android 10+ for file synchronization; arm64-v8a and x86_64; not a Play Store release |
| Windows x86-64 / Linux x86-64 | [0.11.0 preview](https://github.com/ckiikc/musiccraft/releases/tag/v0.11.0) | Separate experimental source/package line; not yet merged with the newest Mac/Android synchronization features |

Each release includes checksums. Read its limitations and verify downloads against `SHA256SUMS.txt`. Intel macOS, physical DAC behavior and every phone/head-unit/NAS combination are not certified by a successful source build.

**New in 0.10.63 / 0.4.2:** reviewed file synchronization, Library-only source selectors on desktop, direct SMB NAS access on Android, recoverable deletions/replacements, and stable track references after phone-file replacement. The actual home NAS test is still pending. See [synchronization](docs/SYNCHRONIZATION.md).

![MusicCraft compact player with stereo VU meters and synthetic metadata](docs/screenshots/miniplayer-vu.png)

## Desktop: library and ReFacets

- Local folders, mounted NAS shares, FTP/FTPS sources and authenticated MusicCraft Android libraries.
- Source-aware browsing, text search and ReFacets artist, album and folder filters. Metadata includes album artist, genre, year, disc and track numbers.
- Incremental catalogue refresh, automatic source checks, background tag indexing and persistent metadata journals. Unreachable or suspended sources are distinguished from genuinely deleted files.
- Phone-network rediscovery derives bounded connected private subnets from the computer's network. Configurable probe intervals: **5 ms, 50 ms, 350 ms or 1 second**. Legacy FTP discovery offers manual server entry when discovery fails; network speed and router behavior still determine results.
- Paired MusicCraft phones use service discovery and verified identity instead of requiring a copied link or a permanent IP address. They reconnect when sharing becomes available, without reopening pairing or restarting the player.
- Album membership uses source, album and album artist, including the phone's flat catalogue. An audio endpoint is not treated as one enormous album.
- Double-clicking track-title text starts the track instead of selecting the text. Queue and ReFacets reveal the current track without clearing active filters or changing the queue order.
- The playing row has two small indicators driven by the existing stereo peak telemetry. Selection follows accepted playback transitions; a separate row deliberately selected for browsing keeps a neutral highlight.
- Finder/Open With support for warm and cold app launches. Multiple delivered files form an ordered listening selection with automatic advance. File associations allow choosing MusicCraft as a default handler; installation does not change that choice automatically.

## Desktop: playlists, queue and ratings

**Library, Playlists and Playback queue are separate views.** Browsing or editing a playlist does not silently replace an active listening queue.

- Create, rename, duplicate and delete saved playlists; add selected tracks or the current filtered ReFacets result.
- Search playlist contents, reorder/remove tracks, and start an explicit listening selection.
- Import M3U/M3U8 and export M3U8 playlists. Relative paths and Unicode names are handled; credential-bearing URLs are rejected.
- Create dynamic playlists with **all/any** rules on **genre, artist, album, title, year, rating and play count**. Text rules support matching/exclusion; numeric rules support equality and minimum/maximum bounds.
- Preview, edit and duplicate dynamic rules. Results refresh when the library, relevant metadata, votes or qualified listening counts change. A queue already playing remains its own snapshot.
- Visible, editable stars next to tracks; changing a rating does not start the song. A context menu remains available. Ratings currently use **whole stars from 0 to 5**.
- Read existing `VOTE`/`RATING` tags with an explicit numeric scale, including 0–5 and 0–100. Ambiguous/conflicting values are not guessed. Explicit MusicCraft ratings, including zero, take precedence over imported values.
- Write rating changes back to supported **FLAC/MP3** tag layouts while preserving audio bytes and unrelated metadata. Unsupported or ambiguous layouts remain unchanged/pending rather than being rewritten broadly.
- Paired phone rating edits require approval in MusicCraft Android and any Android media-write prompt. Metadata journals and verified backups support interrupted-write recovery. This approval is independent from the optional folder-sync permission.
- Qualified play counts are local to each installation; Mac and phone listening history are not automatically combined. See [rating semantics and recovery](crates/musiccraft-tags/README.md).

## Desktop: playback and audio

- Play/pause, previous/next, seeking, volume, repeat, sequential playback, shuffled tracks and shuffled albums.
- Persistent shuffle cycles and prepared next tracks for gapless playback where the format/output path permits it.
- Selectable audio devices and shared or exclusive CoreAudio output on macOS.
- Optional source sample-rate following, integer PCM handling and ReplayGain controls. Unsupported exclusive configurations report an error.
- Authenticated network streaming, buffering, bounded read/retry behavior and cached files. A verified route is retained across audio ranges so expired DNS does not interrupt an already identified phone source.
- Background playback remains independent from window focus. Rendering uses visibility-aware work to reduce CPU; keeping a visible player on another display does not require it to own keyboard focus.
- Source failures, interrupted network reads and decoder failures are distinct from a clean end of track. Playback has no listening-time limit.

### Audio formats and the separate extended-codec preview

| Release line | Formats / transport |
| --- | --- |
| Mac 0.10.63 | FLAC, WAV, AIFF/AIF, ALAC/M4A, MP3, AAC, Ogg/Vorbis and supported MP4 audio containers, subject to the actual codec/container |
| Android 0.4.2 | Formats supported by Media3 and the device's Android decoders; this is not a promise that every desktop format plays on every phone |
| Windows/Linux 0.11.0 preview | Adds APE, WMA, WavPack, Opus and DSF/DFF DSD-to-PCM through a bundled limited FFmpeg runtime; adds an explicit DSF/uncompressed DFF DoP path for compatible exclusive-output DACs |

The 0.11.0 preview includes WASAPI exclusive output on Windows and direct ALSA output on Linux. Its portable ZIP/NSIS and tarball/Debian packages have their own prerequisites and unresolved hardware checks. See the [tagged porting and codec guide](https://github.com/ckiikc/musiccraft/blob/v0.11.0/docs/PORTING.md).

**DoP is experimental and is not raw ASIO/native USB DSD.** That latter transport is not implemented. DoP requires a compatible DAC, integer exclusive transport and supported rates; it bypasses software gain, ReplayGain and resampling and fails explicitly when the requirements cannot be met. DRM WMA, WavPack correction sidecars and compressed DST direct-DoP have additional limitations documented in the tagged guide. The newer 0.10.63 Mac package does not automatically include the separate 0.11.0 extended-codec/DoP branch.

Source tests establish PCM/format and framing behavior, not physical bit-perfect output, acoustic gaplessness or DAC certification.

## Desktop: player views, artwork and windows

- Full library and compact player, with independent remembered window sizes and an always-on-top option.
- **Six player visuals:** vinyl, CD, cassette, reel-to-reel, stacked stereo VU meters and side-by-side stereo VU meters. The central tab reflects the selected visual.
- Artwork remains recognizable on vinyl, CD, cassette and reel-to-reel views. Reel motion uses continuous state; analogue needles use existing L/R telemetry and are not calibrated VU instruments.
- Optional automatic visual selection based on track year; the chosen appearance persists.
- Collapsing the side sections retains the selected central tab. First-use volume dragging and window-drag handling account for focus changes.
- Embedded artwork, local folder artwork and optional online cover lookup. Saving a missing album cover is optional and does not overwrite an existing cover.
- Double-click artwork to open a large independent cover window, with front/back/booklet navigation when images exist and controls that disappear over the image.
- Artwork-window position is remembered across sessions and multiple screens; restored geometry is constrained to available displays.
- Independent settings, user-guide and other auxiliary windows can extend beyond the main player's bounds. The cover window has an opaque native title bar. About is raised above a pinned player.
- Native macOS application menus, artist biographies, plain lyrics and synchronized lyrics when a provider has a suitable result.
- Desktop interface and built-in guide in **23 languages, including Vietnamese**, with the language preference retained across launches. New synchronization sections currently have Italian/English text and English fallback elsewhere; editorial review is still needed.

![MusicCraft appearance settings](docs/screenshots/settings.png)

## Android: library, player and playlists

The Italian mobile interface separates **Search, Playlists, Player and Synchronization**. Search does not change the listening queue. The standalone Player supports portrait and landscape layouts.

- MediaStore library with tracks, albums, artists and search, including indexed shared/removable storage.
- Play now, play next, add to queue, add to a saved playlist and rate a track. Replacing an existing queue asks for confirmation.
- Persistent saved playlists, manual ratings and listening statistics. Queue tools play, move, remove or save tracks.
- A playlist home with Resume listening, suggested mixes and separate Dynamic/Saved lists; a mini player opens the full Player.
- Full artwork, metadata, seeking, transport and upcoming tracks. Secondary landscape content scrolls while transport stays accessible.
- One Media3 player/service for the app, notification, system controls and Android Auto. Audio continues in the background and with the screen off, subject to Android/device power management.
- Audio-focus and headphone-disconnection handling. Screen rotation or Activity closure does not create a second player. Process restart restores queue/position **paused**, without starting music automatically.

### Android dynamic mixes

- Eight initial templates and an editable criteria/preview screen.
- Choose songs, albums or artists; filter by genre, text, year/range/decade, rating and played/unplayed/partly played state.
- Inclusive minimum/maximum qualified play counts combined with ratings. Maximum zero selects never-played tracks.
- Ordering by random selection, total/average counts, total/average rating, number of rated tracks or recent listening.
- A bounded producer adds tracks ahead, avoids adjacent/upcoming duplicates and retains a limited history. **Stop adding automatically** preserves the existing queue.
- Rules and statistics are MusicCraft data, inspired by dynamic-playlist workflows; this is not an LMS connection or an import of LMS ratings/history.

### Android artwork library

- Priority: embedded per-file artwork, identifiable local album artwork, retained artwork, then optional online lookup.
- A persistent private **SQLite index plus deduplicated JPEG store** keeps covers outside Android's disposable cache. Album aliases share identical images; heterogeneous folders retain per-track artwork.
- Changed files/metadata receive new associations. Positive results are reused; negative results, transient errors, rate limits and Retry-After are remembered with bounded retries.
- Optional MusicBrainz/Cover Art Archive lookup is disabled by default, conservative, paced and on demand. It does not scan the entire collection online or guarantee a match.
- Covers are not written into the music directories. Storage is bounded at 256 MiB without automatically discarding retained artwork; clearing app data/uninstalling removes this private library.

## Android Auto and per-car queues

- Browse **Now playing, Mixes, Playlists and Library**, with intermediate routes on hosts allowing fewer tabs. The head unit controls templates, list limits and artwork rendering.
- Show the active playlist/profile and current queue through the media session. Start an existing saved playlist or dynamic mix directly from the car display, without picking up the phone.
- Media-browser search and media-session voice-search support; notification/system transport controls use the same player.
- Separate **Phone, Unrecognized car and named-car queues**, each retaining order, track/position, playlist name, shuffle/repeat and dynamic-producer state.
- Optional Bluetooth recognition associates a known paired head unit with a named car. Android Auto connection plus one unambiguous match selects the profile; manual selection remains available.
- Nearby-device permission is optional on Android 12+. No Bluetooth scanning or location permission is requested. An ordinary headphone connection alone does not select a car.
- Switching/restoring profiles does not autoplay. Shared playlists/ratings remain available without duplicating music files.
- Configure cars/Bluetooth and create/edit mix criteria on the phone; driving controls only start/select existing content. Protocol/emulator checks do not replace testing with a real head unit.

See [Android setup and build](android/README.md) and [car queues](android/CAR_QUEUES.md). A sideloaded test APK may need **Unknown sources** in Android Auto's developer settings; a Play Store release is separate work.

## Pair a phone and stream to the computer

1. On Android press **Condividi → Attiva**. The visible status distinguishes Off, Active, Suspended and waiting for Wi-Fi.
2. Choose **Abbina computer**. On the Mac add a MusicCraft Android source, select the discovered phone and enter its four-digit code.
3. Approve on the phone. Later connections reuse the pairing, even if the local IP changes. A complicated URL or manually maintained IP is not required.

Each installation generates its own Keystore HTTPS identity and pairing credentials. Reads use a pinned certificate; macOS retains credentials in Keychain. Sharing pauses during phone playback and offers revocation. Starting sharing is an explicit user action, including after an update stops the previous service. No separate FTP app is required. Ordinary sharing remains read-only; rating edits and folder synchronization each have their own approval flow. See [the sharing guide](docs/SHARING.md).

The desktop can also expose a selected collection through its original-file HTTP server and control an **external Lyrion Music Server**, including compatible players such as piCorePlayer. The standard public package excludes the optional LMS runtime; see [multiroom](docs/MULTIROOM.md).

## Reviewed NAS ↔ phone file synchronization (experimental)

- A desktop **Synchronization** tab uses only sources already in Library, shown by name. Choose origin A, destination B and **A → B, B → A or bidirectional**.
- Mounted folders/NAS and paired MusicCraft Android are supported. Legacy FTP sources are shown as unavailable for writable folder synchronization.
- Preview copies, updates and deletions; resolve every conflict before applying. One-way mode mirrors its origin, including destination-only deletions. Bidirectional mode compares against the last fully verified baseline.
- Content fingerprints, root identity and stale-preview checks avoid selecting winners merely by timestamps. Changed-both-sides and deletion-versus-edit conflicts require a choice.
- Verified staging and exclusive commits retain replaced/deleted originals under `.musiccraft-sync/recovery`. Interrupted runs need a new preview; recovery is currently manual, not whole-run Undo.
- Android has its own direct signed **SMB2/3** NAS connection, with host/share/subdirectory/domain/user configuration and a password held only for the current screen.
- A native writable folder grant and a separate opt-in computer permission are required. Every computer plan also requires approval of its exact preview on the phone. Sync runs through a user-started data-sync foreground service; startup never silently resumes writes.
- Complete selected roots are synchronized, including artwork and other non-hidden files; this is independent from search filters. Empty directories are not mirrored. Both roots need space/access for control and recovery data.
- Phone replacements retain playlist/stat identifiers; per-file versions invalidate old desktop audio/tag caches on catalogue refresh. Queues, playlists, ratings, car profiles and pairing stores remain separate.

Limits include 100,000 files, 64 directory levels and 8 GiB per phone transfer. Android synchronization requires Android 10+ and the native ExternalStorageProvider; arbitrary cloud document providers are unsupported. Fingerprint metadata can be reused for up to 24 hours, while destinations of planned changes are rechecked before mutation. **Real NAS compatibility, long disconnections and volume-specific behavior remain to be tested.** Read [synchronization and recovery](docs/SYNCHRONIZATION.md) before using personal files.

## Build and test

Desktop builds have been checked on Apple silicon with Rust **1.97.1** and Xcode Command Line Tools. Use at most two Cargo jobs:

```sh
cargo run --release --locked -j 2
# Separate test library/settings:
cargo run --locked -j 2 -- --data-dir /private/tmp/musiccraft-demo
# Experimental public bundle, without LMS:
scripts/package-macos.sh release
```

The public app retains `io.github.ckiikc.musiccraft`, `~/Library/Application Support/MusicCraft/` and its existing public Keychain service. It starts empty on a new installation and does not import the private development profile. An update must keep the application identity; do not replace a private-profile installation with a differently identified public bundle and expect an automatic migration.

Android uses JDK 21, SDK 36, the checked Gradle wrapper, Rust Android targets and the NDK. Set `ANDROID_HOME` or an ignored `local.properties`:

```sh
./android/gradlew -p android :app:assembleDebug :app:testDebugUnitTest :app:lintDebug
```

Install an Android update with the **same certificate/applicationId**; never uninstall or clear data when retaining library settings, playlists, votes, car queues and pairing. Signing keys and SDK paths are not distributed.

Developer gates:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -j 2 -- -D warnings
cargo test --locked -j 2
cargo test --locked --manifest-path crates/musiccraft-sync/Cargo.toml -j 2
cargo test --locked --manifest-path crates/musiccraft-tags/Cargo.toml -j 2
python3 scripts/check-publication.py
python3 scripts/test_app_uuid.py
python3 scripts/test_document_types.py
```

Synthetic FTP, paired HTTPS, SMB/SAF, rating recovery, UI and Android instrumentation checks have explicit prerequisites. Instrumentation runs only in an isolated emulator, never on a personal phone. Ignored private-library/hardware tests are not run by default. See [validation](docs/VALIDATION.md), [architecture](docs/ARCHITECTURE.md), [releasing](docs/RELEASING.md) and [contributing](CONTRIBUTING.md).

## Privacy, dependencies and license

Music files stay on configured devices. Optional online cover, biography and lyrics lookup sends relevant artist/album/title queries to the selected providers. Retrieved media/text is not project artwork and retains its applicable rights/attribution. Provider matches and availability are not guaranteed.

FTP is unencrypted; use FTPS or a network you trust. Keep music servers on a private network and do not forward their ports to the Internet. Do not publish URLs with credentials, pairing material, personal libraries, downloaded metadata or raw diagnostic screenshots. See [security](SECURITY.md).

Project source is [MIT](LICENSE). Original artwork and synthetic fixtures are described in [attribution](ATTRIBUTION.md); Lucide icons and all dependencies retain their licenses. Fonts are loaded locally; no personal fonts are bundled. Binary packages include dependency notices. See [desktop components](THIRD_PARTY.md), [Android components](android/THIRD_PARTY.md) and the [pinned inventory](docs/DEPENDENCIES.tsv). Optional LMS redistribution is not included in the standard package.

Maintained by [@ckiikc](https://github.com/ckiikc).
