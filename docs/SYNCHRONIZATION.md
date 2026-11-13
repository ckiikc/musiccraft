# File synchronization (experimental)

The **Synchronization** tab copies files between two existing Library sources. The selectors use their Library names; there is no second folder browser or independent source catalogue. Add or edit sources in Library. Mounted local/NAS directories and paired MusicCraft Android devices are supported; legacy FTP entries are visible but unavailable for file synchronization.

Choose origin A, destination B and A → B, B → A or both directions. Generate a preview, resolve every conflict, then explicitly apply it. A one-way operation mirrors its origin, including destination-only deletions. Bidirectional mode uses the last completely verified baseline. Different initial contents and deletion-versus-edit changes require an explicit choice. Timestamps never select the winning version.

This operates on the complete selected roots, including artwork and other non-hidden files, independently of Library search filters and playback queues. Catalogue refresh remains a separate read-only operation. Empty directories are not mirrored. Recovery is retained under `.musiccraft-sync/recovery`; no recovery purge or automatic destructive retry is performed. An interrupted operation needs a fresh preview. Recovery currently requires manual restoration; this is not a whole-run Undo feature.

## Android

On Android 10 or later, use **Synchronization** to grant one music directory on the phone or SD card and explicitly enable computer access. Keep the existing sharing service enabled. Review and apply every plan on the computer, then approve its exact file preview on the phone. The directory grant is remembered, and the existing pairing is retained. Its existing pairing credential and pinned HTTPS certificate authenticate the connection; no new pairing is required. Ordinary sharing remains read-only when this optional permission is off.

For direct NAS access, configure SMB host, share, optional subdirectory/domain and username. The password is held for the current screen only and is not saved. SMB2/3 signing is required. Android uses its native ExternalStorageProvider grant; other document/cloud providers are not supported. An explicitly started data-sync foreground service keeps the transfer independent of the audio service. App termination preserves originals and journals, but does not resume mutations automatically.

## Verification and boundaries

- Complete snapshots, root identities, content fingerprints, stale-preview checks, exclusive commits and recoverable replacement/deletion.
- Initial indexing reads each file. Unchanged fingerprint signatures can be reused for up to 24 hours; planned destinations are invalidated and rechecked from bytes before mutation.
- Limits: 100,000 files, 64 directory levels and 8 GiB per phone transfer. Control metadata requires write access, including for the first preview. Recovery space must remain available.
- The filesystem adapter requires an exclusive rename primitive. Unsupported volumes refuse the operation. macOS local volumes, loopback SMB on Android and the paired HTTPS path have separate tests; the user's physical NAS remains to be tested at home.
- Existing Library, playlist, listening statistics, car queues and pairing stores are retained. An additive identity database preserves playlist/statistics references when Android replaces a file; per-file sync versions invalidate desktop audio/tag caches on catalogue refresh.
- New interface/manual strings are Italian and English; other existing languages currently use English fallback for these new sections. Existing translations are retained.

See the [shared core](../crates/musiccraft-sync/README.md) for planner semantics and focused test commands. No personal NAS address, folder path, credential or pairing token is embedded in the application.
