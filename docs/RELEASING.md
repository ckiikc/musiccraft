# Release 0.10.63 / Android 0.4.2

Experimental macOS/Android file synchronization, independent from the Windows/Linux 0.11.0 preview. Public Apple-silicon builds retain `io.github.ckiikc.musiccraft`, the existing public MusicCraft profile and an ad-hoc signature. LMS is excluded; the private legacy profile is never bundled or imported. Not notarized.

Android retains `it.ckiikc.musiccraft`, versionCode 11 and the existing test certificate. Install as an update; do not uninstall or clear data. Native ABIs: arm64-v8a and x86_64. Music playback requires Android 8+; file synchronization requires Android 10+ and an explicitly selected writable directory. Each computer plan requires phone approval. Direct SMB credentials are not saved.

Build with `scripts/package-macos.sh release` and JDK21 `android/gradlew -p android assembleDebug testDebugUnitTest lintDebug`. Rust Android targets and the NDK are prerequisites. Distributed native paths are remapped. Exact dependency notices are included. SHA256SUMS accompany the separate prereleases.

Read [synchronization](SYNCHRONIZATION.md) before use. Copies, updates and deletions require preview; conflicts need resolution. Recovery is manual. The actual home NAS, long-term mount/disconnect behavior and cross-platform support remain unverified. No real media, configuration, signing key, private snapshot or raw diagnostic report is distributed.
