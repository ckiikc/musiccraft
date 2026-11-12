# Releases

## Local desktop 0.11.0 preview

Windows x86-64 (per-user NSIS installer/portable ZIP) and Linux x86-64
(Debian package/tarball) are prepared locally. See [porting](PORTING.md) for
build recipes and [validation](VALIDATION.md) for completed checks and native
Windows/DAC limitations. These artifacts are unsigned experimental packages,
not an ArtCraft-approved release. No new GitHub release or installed Mac update
is implied by their generation. Android retains its independent version.


MusicCraft 0.10.50 is an independent, experimental macOS release. Community-app
approval remains a maintainer decision in ArtCraft Launcher issue #1247.

## Current public package

- Apple silicon (arm64) macOS only; minimum advertised macOS version 12.
- Stable public app ID: `io.github.ckiikc.musiccraft`.
- Fresh profile: `~/Library/Application Support/MusicCraft/`; the private
  development app's legacy profile is not imported.
- Ad-hoc signature, no Developer ID signature and no notarization.
- Optional LMS runtime excluded; external LMS control remains available.
- Third-party notices and exact source URLs/checksums are bundled.

Build with `scripts/package-macos.sh release`. To use a shared Cargo build
cache, set `CARGO_TARGET_DIR`; this changes only the build output directory.
The packager never installs or publishes the generated app.

After the documented local checks and native smoke test, archive the app:

```sh
ditto -c -k --sequesterRsrc --keepParent dist/MusicCraft.app dist/musiccraft-0.10.50-macos-arm64-experimental.zip
cd dist
shasum -a 256 musiccraft-0.10.50-macos-arm64-experimental.zip > SHA256SUMS.txt
```

Publish an experimental GitHub prerelease with that ZIP and checksum file,
pointing its tag to the reviewed source commit. Public sources and the lockfile
must match the package. Keep personal profiles and local build logs outside Git.

## ArtCraft Launcher consideration

The launcher requires explicit approval of the repository and the publisher's
Developer ID team for managed macOS installation. An ad-hoc build does not meet
that requirement. This release supports source review and manual experiments;
it is not an approved launcher installation source.

Intel macOS, other operating systems, signing/notarization and combined LMS
redistribution require separate work and validation. Do not advertise them as
supported based solely on shared Rust dependencies or a passing local test.

## Android test distribution

Android source lives under `android/`; it has an independent version, currently 0.3.0 (versionCode 4), and stable package ID `it.ckiikc.musiccraft`. Compile with JDK 21 and Android SDK 36 using the checked Gradle wrapper. The wrapper and distribution checksums are pinned. `android/README.md` documents the build and emulator-only tests.

The current public APK is an explicitly labelled debug/test prerelease. It contains no synthetic test audio (that is in the separate instrumentation APK), private library or device credentials. Signing keys, local.properties, caches and APKs are excluded from Git. A locally generated debug key is not a long-term distribution identity: an update requires the same signing certificate, and a future release channel needs a separately managed publisher key. Do not assume a clean CI debug APK can update the downloaded test APK.

This is not a Google Play listing or an approved Android Auto distribution. Android Auto discovery for sideloaded builds may require its own developer options. Runtime permissions must be granted by the user on physical devices.
