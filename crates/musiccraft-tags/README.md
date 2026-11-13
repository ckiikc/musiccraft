# MusicCraft rating tags

Shared Rust engine used by desktop and Android JNI. No external service receives paths, ratings or audio.

## Semantics

- Reading and indexing never write music files. Original tag text is retained separately from MusicCraft's private ratings.
- A manual MusicCraft rating always wins, including an explicit zero. Loading earlier private statistics preserves their existing ratings. Play counts are independent and are not imported from unrelated tag counters.
- Numeric VOTE/RATING values require an explicit scale choice: 0–5 or 0–100. The default accepts only self-described values such as `4/5` or `80/100`. Values are converted only to exact whole stars; no rounding, clipping or guessing. Duplicate/conflicting ratings are unknown. Unknown/unread ratings do not match desktop rating rules; Android excludes them when a restrictive rating range is selected.
- Star edits are saved privately and queued for tag synchronization. A pending/failed write is never reported as successful. Desktop rows offer a retry through the context menu. File access, scale conflicts and unsupported structures leave the original intact.

## Safe write scope

The current engine reads/writes FLAC Vorbis comments and MP3 ID3v2.3/v2.4 TXXX VOTE/RATING fields. It updates all existing VOTE/RATING fields, keeping their spelling and explicit denominator; otherwise it adds RATING using the chosen scale. Other formats and POPM/application-specific scales are not silently converted.

Only fixed-size metadata prefixes are generated. Every other frame/comment/block and the complete audio payload remain unchanged. Existing whole-star ratings normally need no extra space. Adding a field requires adequate ID3 padding or an immediately following FLAC PADDING block. Oversized, malformed, flagged/unsynchronised ID3, conflicting or unsupported layouts are rejected. The metadata limit is 8 MiB; read-only indexing seeks over artwork rather than downloading it.

Desktop creates and syncs a complete private original backup, writes a same-directory copy, verifies the rating and identical audio/trailing bytes, checks that the original did not change, then renames atomically. Symlinks, hard links, read-only files or ownership/attribute preservation failures are refused. macOS ACLs and extended attributes are preserved. Backups are retained under the private profile's `rating-backups` directory; they are complete audio files and consume disk space. They are not automatically deleted.

Android uses a system-granted MediaStore descriptor. Before the first write it durably journals both metadata prefixes. It writes only that prefix, without truncating/moving/re-encoding audio, syncs and rereads it. Interrupted writes are recovered before the sharing server starts. A prefix inconsistent with either journal version is not overwritten; recovery must succeed before another edit. Android does not promise filesystem-atomic replacement, but keeps a durable recovery record until validation succeeds.

## Phone protocol

Existing paired/pinned HTTPS authentication is required for `POST /v1/ratings/{song-id}` with `{rating, scale, request}`. IDs resolve only through MediaStore; no upload endpoint or filesystem path is accepted. Requests are bounded, persisted and idempotent by request ID. The phone displays the exact song and rating and requires explicit approval, followed by Android's write authorization where applicable. Requests cannot bypass approval. `GET /v1/ratings/{song-id}/{request-id}` reports waiting/done/error; desktop only clears pending state after confirmed completion. Identical already-applied values require no additional write. File serving, playback and queue APIs remain separate.

## Development

`cargo test --locked -j 2 --manifest-path crates/musiccraft-tags/Cargo.toml`

Android builds require Rust's `aarch64-linux-android` and `x86_64-linux-android` targets and an Android NDK. Gradle runs `android/scripts/build-rating-tags.py`. Set ANDROID_HOME outside the standard macOS SDK location. The APK currently contains arm64-v8a and x86_64 native libraries; other ABIs are not claimed.

Synthetic FLAC/MP3 fixtures in Android instrumentation assets are generated from a 440 Hz sine by FFmpeg. No personal library files or settings are used by tests.

External tag edits can be re-indexed with **Refresh audio tag ratings** in desktop Settings. This does not clear personal overrides or change a running queue. Android's rating cache is keyed by MediaStore file size/modification time; library refresh revisits changed items. Large-library imports persist rating changes in batches rather than queueing a complete statistics-file rewrite for every song.
