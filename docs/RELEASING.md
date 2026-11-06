# Releases

MusicCraft 0.10.43 is an independent, experimental macOS release. Community-app
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
ditto -c -k --sequesterRsrc --keepParent dist/MusicCraft.app dist/musiccraft-0.10.43-macos-arm64-experimental.zip
cd dist
shasum -a 256 musiccraft-0.10.43-macos-arm64-experimental.zip > SHA256SUMS.txt
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
