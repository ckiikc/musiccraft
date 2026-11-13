# Architecture

The desktop is one Rust binary with internal modules; bounded tag handling and synchronization planning live in two shared helper crates.

- `app/window_drag.rs`: preserves the original AppKit left mouse-down for delayed miniplayer dragging, validates its window geometry and cancels interrupted gestures. Its monitor is application-local; optional event/geometry tracing exists only in debug builds with an explicit isolated profile.
- `app/`: egui views, shared controls, persisted user settings, filtered library and UI event handling.
- `audio.rs`, `integer.rs`, `exclusive.rs`, `dsp.rs`: playback planning, PCM decoding, macOS device leases/output and optional processing. Low-level CoreAudio calls are isolated in the macOS backend and contain documented unsafe FFI.
- `library.rs`, `facets.rs`, `shuffle.rs`, `history.rs`: track identity, metadata, browsing and playback selection. FTP identity canonicalization decodes a path segment once and keeps different sources separate.
- `network.rs`, `sync.rs`, `discovery.rs`, `remote_tags/`: authenticated remote access, incremental directory discovery, bounded phone rediscovery and one paced metadata-reading lane. Audio work has priority. Metadata cancellation must terminate readers rather than return a permanently retryable Interrupted error.
- `jobs/gallery.rs`: bounded folder artwork enumeration and image loading for the native cover/booklet viewer.
- `cache.rs`, `jobs.rs`, `credentials.rs`: bounded transfer cache, background jobs and macOS Keychain. Worker results are generation-guarded before adoption by the UI.
- `album_art.rs`, `listening.rs`: bounded online artwork, artist and lyrics queries, with attribution in the UI. Debug/test-only audits must use an explicit private output directory.
- `media_server.rs`, `multiroom.rs`, `embedded_server.rs`: original-file sharing, external LMS control and optional local-runtime lifecycle. The normal publication package excludes that runtime.
- `i18n/`: 22 translated TSV catalogs plus English, system-font fallbacks and localized built-in help.

The public release uses io.github.ckiikc.musiccraft and a separate MusicCraft profile. It contains no migration of personal settings. A future cross-platform port needs explicit platform data directories, credential stores, network process adapters, output backends and packaging; shared Rust dependencies alone do not establish portability.

## Android companion and direct phone source

`android/` is a separate Kotlin/Android project, not a Rust workspace member. Catalog reads MediaStore on a worker. PlaybackService owns the sole ExoPlayer/MediaLibrarySession; PlaybackCoordinator owns the queue and dynamic producer. Activities are controllers, so recreation/rotation does not replace the player. ListeningStore persists private snapshots atomically on a worker; SmartRules is pure Kotlin selection logic.

The user-started Android sharing service uses a foreground notification, a per-installation Android Keystore TLS identity and read-only HTTPS. NSD/Bonjour advertises `_musiccraft._tcp.`. Four-digit short-lived SRP-6a pairing requires explicit phone approval before exchanging the encrypted identity/credential. The desktop's `phone/` and `app/phone_pair.rs` use the same sync/streaming paths as other library sources. macOS Keychain stores credentials; library URLs carry no token. Sharing suspension is distinct from an empty catalog and never triggers destructive pruning.

`open_documents.rs` receives macOS file-browser events and delivers their ordered selection to the existing playback path. The packager declares audio associations without changing the user's default handler.

## Desktop continuity and current-track indicators (0.10.57)

The native macOS menu actions dispatch to the same UI/player paths. Auxiliary windows use independent native viewports. Network audio keeps the initially verified HTTP(S) route for each file; native DNS-SD resolves paired phones to verified local addresses while preserving identity/pin checks. Stream failures remain distinct from clean EOF and decoder failures.

The album-side list identifies the current album within its source rather than using a flat HTTP parent path as the album. Table selection follows accepted Started events unless the user is browsing another row. A two-channel row indicator consumes the existing audio peak telemetry, with no new audio callback, decoder or repaint timer. Its hover-only accessibility marker does not take row clicks.

## Rating interoperability and active-row following

`crates/musiccraft-tags` is a shared Rust library and Android JNI engine for bounded read-only rating inspection and fixed-size FLAC/MP3 metadata patches. Descriptive metadata, raw rating tags and private manual overrides are separate. Explicit scales and conflicts are handled before playlist evaluation; no guessed rating is used by a restrictive rule. Desktop writes preserve a verified backup and replace a same-directory copy atomically; Android journals metadata prefixes and requires phone/MediaStore approval. Existing paired TLS identity protects a narrowly scoped rating API, never a generic file-upload path. Queues remain playback snapshots.

`app/follow.rs` tracks current-source/view/list revisions and resolves an off-screen virtualized row only when needed. It does not send audio commands, clear filters or rewrite queues. Android count bounds are persisted with backward-compatible unbounded defaults.

## Reviewed file synchronization

`crates/musiccraft-sync` builds validated plans from root-bound content snapshots and the last verified baseline. Desktop `file_sync` adapters execute local/paired HTTPS operations off the UI thread. The UI uses only existing Library sources. Android uses the same planner through JNI, SAF ExternalStorageProvider and signed SMB2/3. A separate user-started dataSync service owns direct transfers. Music writes serialize with rating changes; exact remote plans require phone approval. Staged verified bytes, exclusive commits and retained originals protect interrupted operations. A separate additive identity/version database preserves Android playlist/stat keys after file replacement and invalidates desktop cached bytes on catalogue refresh. See SYNCHRONIZATION.md for limits; this is not an automatic NAS backup or whole-run Undo.
