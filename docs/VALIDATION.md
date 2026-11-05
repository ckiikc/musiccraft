# MusicCraft 0.10.41 validation — 2026-11-05

The public source tree was checked separately from the private development
workspace. It uses a fresh MusicCraft profile and the public app identity
io.github.ckiikc.musiccraft.

Completed locally on Apple silicon macOS with rustc 1.97.1:

- 185 ordinary tests passed; 39 tests with explicit device, network, private
  library or multiroom prerequisites were not run indiscriminately.
- All 12 authenticated loopback FTP integration tests passed with synthetic
  audio and artwork, including progressive ranges and booklet browsing.
- Formatting, Clippy all-targets with warnings denied, and diff checks passed.
- Three Mach-O identity tests passed; production and QA UUIDs are distinct.
- The targeted publication scan found no findings, including newly added files.
- The dependency inventory covers 247 pinned packages with declared licenses.
  Dependency notices include font licenses and pinned upstream notices missing
  from some crate archives, plus exact source URLs and Cargo.lock checksums.
- Native debug screenshots were inspected with isolated, synthetic profiles:
  the miniplayer at 520×520 points and the responsive library at 640×520 points.
  No real album artwork, biographies, lyrics or personal libraries are included.
- The title-bar regression reproduces premature StartDrag on mouse-down in the
  previous implementation. The corrected handler passes click, jitter, long
  stationary press, secondary button and one native request per actual drag.
- Persistence covers the six player appearances and older source-review profiles.

These are local results. GitHub Actions source checks are configured separately;
a workflow definition is not a completed CI run. Hardware bit-perfect output,
acoustic gaplessness, Intel macOS, other operating systems and native-language
editorial review require separate evidence. Analog meters visualize existing
peak telemetry rather than calibrated VU measurements.

The public package is experimental, ad-hoc signed and not notarized. It excludes
the optional LMS runtime. See RELEASING.md for its distribution status and the
remaining requirements for ArtCraft Launcher consideration.
