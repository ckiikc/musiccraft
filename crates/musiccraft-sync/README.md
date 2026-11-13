# MusicCraft synchronization core

Shared, content-based preview planner for desktop and Android. This crate does not discover devices or grant storage access. The caller must supply complete snapshots from two authenticated, explicitly selected roots.

## Semantics

- One-way mode mirrors the selected origin, including destination-only deletions in the preview.
- Bidirectional mode merges new files and compares subsequent edits against the last fully verified baseline. Deletion against an edited peer is a conflict. Different contents on the first synchronization require a choice.
- No modification time decides which edit wins. Content fingerprints define equality; metadata is only used to reuse recently computed fingerprints.
- Missing or inaccessible roots, incomplete listings, unsafe paths, case collisions and overlapping roots stop the operation. They are not empty libraries.
- Apply requires the exact reviewed snapshot. Adapters recheck each destination fingerprint, stage and verify incoming bytes, retain replaced/deleted originals, and verify the resulting snapshots before advancing the baseline.
- An interrupted run retains its journal and recovery files. It never advances the baseline or automatically retries a destructive operation. Generate a new preview to reconcile partial completion.

## Storage and limits

The desktop filesystem adapter stores a root identity, fingerprint index and recovery files under `.musiccraft-sync`. The Android adapter keeps indexes in private app storage and recovery files inside the explicitly granted directory. Hidden files are excluded. Empty directory structure is not mirrored. A complete initial fingerprint pass reads the files; subsequent previews reuse unchanged size/modification signatures for up to 24 hours. Planned destinations are invalidated before mutation and checked from their bytes before replacement or deletion.

Maximum: 100,000 files, 64 directory levels, 1,024 bytes per relative path. Symbolic links and special files are rejected by the desktop adapter. SMB refuses reparse points and uses SMB2/3 with signing. The grant on Android is restricted to its ExternalStorageProvider. Other document/cloud providers are not claimed supported.

The recovery directory needs free space and is never purged automatically. No system recycle-bin integration or whole-run atomicity is claimed. Root control metadata requires write access, including during the first preview. Normal playback queues, playlists and file rating scales are outside this protocol.

## Validation

Run `cargo test --locked --manifest-path crates/musiccraft-sync/Cargo.toml` and `cargo clippy --locked --manifest-path crates/musiccraft-sync/Cargo.toml --all-targets -- -D warnings`. Android's emulator-only `syncOnly` instrumentation checks a real loopback SMB server and a separately granted QA directory. Personal NAS/device validation must be reported separately.
