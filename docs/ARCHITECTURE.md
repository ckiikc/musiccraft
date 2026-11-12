# Architecture

The current alpha is one Rust binary with internal modules, not a multi-crate framework.

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

The public release uses io.github.ckiikc.musiccraft and a separate MusicCraft profile. It contains no migration of personal settings. The cross-platform boundary is described in PORTING.md; shared Rust dependencies alone do not establish runtime or hardware validation.

## Android companion and direct phone source

`android/` is a separate Kotlin/Android project, not a Rust workspace member. Catalog reads MediaStore on a worker. PlaybackService owns the sole ExoPlayer/MediaLibrarySession; PlaybackCoordinator owns the queue and dynamic producer. Activities are controllers, so recreation/rotation does not replace the player. ListeningStore persists private snapshots atomically on a worker; SmartRules is pure Kotlin selection logic.

The user-started Android sharing service uses a foreground notification, a per-installation Android Keystore TLS identity and read-only HTTPS. NSD/Bonjour advertises `_musiccraft._tcp.`. Four-digit short-lived SRP-6a pairing requires explicit phone approval before exchanging the encrypted identity/credential. The desktop's `phone/` and `app/phone_pair.rs` use the same sync/streaming paths as other library sources. macOS Keychain stores credentials; library URLs carry no token. Sharing suspension is distinct from an empty catalog and never triggers destructive pruning.

`open_documents.rs` receives macOS file-browser events and delivers their ordered selection to the existing playback path. The packager declares audio associations without changing the user's default handler.

## Desktop continuity and current-track indicators (0.10.57)

The native macOS menu actions dispatch to the same UI/player paths. Auxiliary windows use independent native viewports. Network audio keeps the initially verified HTTP(S) route for each file; native DNS-SD resolves paired phones to verified local addresses while preserving identity/pin checks. Stream failures remain distinct from clean EOF and decoder failures.

The album-side list identifies the current album within its source rather than using a flat HTTP parent path as the album. Table selection follows accepted Started events unless the user is browsing another row. A two-channel row indicator consumes the existing audio peak telemetry, with no new audio callback, decoder or repaint timer. Its hover-only accessibility marker does not take row clicks.

## 0.11.0 platform adapters

`platform/` supplies profile paths, trusted system process adapters, installed system fonts and monitor work areas. `credentials.rs` preserves the macOS service and adds Windows Credential Manager/Linux Secret Service. `phone/mdns.rs` uses in-process DNS-SD; discovered addresses remain untrusted until existing SRP/TLS identity checks accept them. `open_documents/instance.rs` combines an OS file lock with a per-profile, authenticated, bounded loopback handoff so file-manager requests cannot race profile writes.

`exclusive/portable.rs` owns WASAPI/ALSA clients on one worker and reuses the byte renderer/ring queues. It preserves partial writes and rejects transport changes within a stream. `decoder/` selects native decoding, bounded FFmpeg PCM output, or direct DSF/DSDIFF DoP data. Transport is explicit in decoded blocks; DoP forces integer exclusive validation and renderer-owned markers, bypassing all DSP. Sources and notices for the minimal codec executable ship alongside it. See PORTING.md for driver, cache and format limits.
