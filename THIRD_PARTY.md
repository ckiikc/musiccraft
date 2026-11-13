# Third-party components

MusicCraft's own source is MIT. Rust dependencies retain their individual licenses; the lockfile pins the resolved versions. `docs/DEPENDENCIES.tsv` records package names, versions, declared SPDX license expressions and source references for the checked macOS dependency graph. This inventory is not a legal compatibility opinion or a vulnerability audit. Preserve applicable notices when distributing binaries.

The macOS packager collects dependency license texts, pinned upstream notices
and exact crate source URLs/checksums in `Contents/Resources/ThirdPartyLicenses/`.
The MPL-2.0 Symphonia components are unmodified; their source versions are in
that inventory. Pinned supplemental license provenance is in `docs/licenses/manifest.json`.

Lucide icon licenses and attribution are retained under `assets/icons/`. eframe/egui supplies its normal bundled UI font under its dependency license. Supplemental fonts are loaded from the user's system and are not copied into the app.

## Optional Lyrion Music Server

The Rust multiroom controller communicates with LMS through its public JSON-RPC interface. The default package does **not** contain LMS, Perl, firmware, upstream graphics or vendor archives. External LMS remains available. This repository contains the integration code and a separately invoked local-use runtime preparation recipe.

The local recipe pins the official LMS 9.1.1 macOS download and its SHA-256. It retains upstream notices and changes only the nested bundle's ad-hoc signature. Its generated `vendor/` and application bundle are excluded from Git. That recipe is not a redistribution clearance.

The [upstream License.txt](https://github.com/LMS-Community/slimserver/blob/public/9.1/License.txt) distinguishes GPL-2.0 Perl code, dependencies with their own licenses, and ancillary material with restrictions, including firmware and graphics. Before any combined public binary release, inspect the exact payload, remove or obtain permission for restricted components, and provide the applicable notices and corresponding sources. No such combined binary release is part of this preparation.

- [Official runtime download](https://downloads.lms-community.org/LyrionMusicServer_v9.1.1/LyrionMusicServer-9.1.1.dmg)
- [LMS sources and notices](https://github.com/LMS-Community/slimserver/tree/public/9.1)
- [Platform build sources](https://github.com/LMS-Community/slimserver-platforms)
- [Vendor build sources](https://github.com/LMS-Community/slimserver-vendor)

## Android companion

AndroidX Media3, Kotlin, Guava and Bouncy Castle retain their notices; see [android/THIRD_PARTY.md](android/THIRD_PARTY.md) and the notices included in the APK. The Gradle wrapper is Apache-2.0. Three synthetic FLAC test fixtures are original CC0 tones, hash-pinned by the publication scanner and excluded from the normal app APK. No LMS source, interface assets or music recordings are copied into Android.

## File synchronization

The shared planner is MIT source under `crates/musiccraft-sync`. Android direct NAS access uses SMBJ and its dependency notices; see [Android notices](android/THIRD_PARTY.md). The standard macOS package continues to exclude LMS.
