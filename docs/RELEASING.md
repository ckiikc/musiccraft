# Release 0.10.62 / Android 0.4.1

The macOS/Android release line is based on the public macOS source branch, separately from the 0.11.0 Windows/Linux experimental tag. The public Apple-silicon app retains `io.github.ckiikc.musiccraft`, a fresh MusicCraft profile and an ad-hoc signature. It excludes the optional LMS runtime. Developer ID signing/notarization and ArtCraft approval are not implied.

The Android test APK retains `it.ckiikc.musiccraft`, versionCode 10 and the existing test signing certificate. Install as an update; never uninstall or clear app data to retain queues, playlists and pairing. Keystore files and private configuration are not distributed. The APK contains arm64-v8a and x86_64 native libraries.

Tag writing is intentionally limited to supported FLAC/MP3 layouts with fixed-size metadata patches; unsupported or ambiguous cases stay unchanged/pending. Phone edits require explicit MusicCraft and Android media-write approval. Qualified play counts are local to each installation. See the shared engine README and validation notes.

Build with `scripts/package-macos.sh release` and `JAVA_HOME=<JDK21> ANDROID_HOME=<SDK> ./android/gradlew -p android assembleDebug testDebugUnitTest lintDebug`. Android also needs the installed Rust Android targets and NDK. Native build paths are remapped to prevent publication of builder home/workspace paths.

Packages and SHA256SUMS are attached to separate experimental desktop and Android releases. No private legacy profile, pairing token, library, personal screenshot, raw diagnostic report or vendor LMS payload belongs in either archive.
