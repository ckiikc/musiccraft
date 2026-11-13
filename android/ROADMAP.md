# Android roadmap

## 0.1.0 — first test version

MediaStore library; tracks/albums/artists/search; background playback through Media3 with notification and system controls; Android Auto browsing service; signed debug APK. See [VALIDAZIONE.md](VALIDAZIONE.md) for actual checks and limitations.

## 0.2.1 — direct sharing

Read-only HTTPS server, local discovery and pairing with four digits and phone approval. The server suspends during local listening. Credentials are generated per installation; no personal configuration is distributed.

## 0.3.0 — navigation and playlists

Separate Search, Playlists and Player views; standalone portrait/landscape Player; explicit editable queue, saved playlists and dynamic rules with local ratings and history. Queue and position restore paused; no automatic audio startup. Dynamic mixes are exposed through the Android Auto browser. See README and VALIDAZIONE for semantics and limitations.

## Next steps

- Physical-phone testing with audio permission and a personal library; screen-off playback, headphones/Bluetooth and phone-call interruptions.
- Visible Android Auto launcher testing in a car or Desktop Head Unit.
- Selectable folders, removable storage handling and further hardware interruption testing.
- Translations and MusicCraft visualizations; synchronization and network sources as separate work.

## 0.3.1 — discoverable sharing

Direct access and persistent status in Search, Playlists and Player. The panel follows service startup without closing and offers pairing when the connection is ready. No automatic server startup or identity/pairing changes.

## 0.3.2 — recognizable playlists and per-car queues

Playlist/profile titles in the audio session; Now playing/Cars and queues/Saved playlists categories in the Android Auto browser; starting mixes from the car display. Independent phone and car queues retain position, playback modes and generator state, restoring paused. Profiles can be associated once with the head unit's Bluetooth device; optional recognition during Android Auto requires no scanning or location, with manual fallback when identification is unavailable. See VALIDAZIONE and CAR_QUEUES for actual checks and hardware limitations.

## 0.3.3 — Bluetooth cleanup without interrupting sharing

Bluetooth proxy cleanup is separate from disconnection callbacks: resources are detached before calling Android. Cleanup is repeatable and callbacks are serialized on the main thread, including late callbacks. Fixes a whole-process crash during audio-service shutdown that also stopped the music server. No changes to credentials, car profiles, permissions or sharing startup behavior.

## 0.3.4 — HTTPS suspension without a crash

A dedicated worker closes TLS connections during suspension, network changes and service shutdown. Resources are detached before closing; cleanup events are batched, and already submitted work drains during shutdown. Removes the NetworkOnMainThreadException observed with an active TLS client when phone audio began. Identity, authentication, read-only access and sharing consent are unchanged.

## 0.4.0 — Continuity and artwork storage

Search/Playlists/Player navigation with a consistent visual hierarchy, queue resume, separate mixes and saved playlists, upcoming tracks, dedicated car-profile view and adaptive landscape layout. Android Auto separates Now playing, Mixes, Playlists and Library, preserving routes for hosts with fewer tabs.

Per-file local artwork, permanent deduplicated SQLite/JPEG storage with album aliases, explicitly enabled MusicBrainz/Cover Art Archive lookup, conservative matches and bounded retries. No change to the listening-state format, audio engine or pairing. See README and VALIDAZIONE for limits and verification.

## 0.4.1 — rating-tag interoperability

Shared Rust metadata engine; persistent read-only rating index; manual-vote precedence; explicit 0–5/0–100 scale selection. Authenticated, bounded, persisted desktop edit requests with phone and MediaStore approval, metadata-prefix verification and interrupted-write recovery. Only supported FLAC/MP3 layouts are written; queues, car profiles and pairing identity are retained. Emulator validation is separate from physical-device validation.
