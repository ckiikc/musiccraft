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

The public release uses io.github.ckiikc.musiccraft and a separate MusicCraft profile. It contains no migration of personal settings. A future cross-platform port needs explicit platform data directories, credential stores, network process adapters, output backends and packaging; shared Rust dependencies alone do not establish portability.
