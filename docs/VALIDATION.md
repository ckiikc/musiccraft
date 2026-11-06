# MusicCraft 0.10.43 validation — 2026-11-06

The public source tree was checked separately from the private development
workspace. It retains the public app identity `io.github.ckiikc.musiccraft`,
an empty initial MusicCraft profile and its own Keychain service.

Completed locally on Apple silicon macOS with Rust 1.97.1:

- 191 ordinary tests passed; 39 tests requiring explicit devices, external
  networks, private libraries or multiroom prerequisites remained excluded.
- All 12 authenticated loopback FTP integration tests passed with synthetic
  audio and artwork. No saved network sources or credentials were used.
- Formatting, Clippy all-targets with warnings denied and diff checks passed.
- Three Mach-O identity tests passed. Public, local and QA identities remain distinct.
- A publication scan of the tracked and new public files found no findings.
- The exact public release bundle passed deep signature verification and a
  native smoke test with an empty isolated profile. Its executable contains
  neither the local home path nor the private workspace name.
- Dependencies remain unchanged: 247 pinned packages, with bundled notices,
  exact source references and checksums.

## Settings and graphic players

The settings sidebar covers Interface, Audio, Library and Multiroom. Pointer
input tests switch all four pages at 660×460, 680×620, 880×720 and 1100×780
points. Native screenshots were inspected for each page, with additional
Interface checks in light/dark themes, 130% text size and Japanese. Only
synthetic settings screenshots are included in this repository; no detected
private-network addresses or personal profiles are published.

Appearance by year tests cover inclusive cutoffs, custom media mappings,
invalid/missing years, sanitized loaded settings, older-profile defaults and
serialization. Toggling the actual checkbox changes the visual without sending
an audio command. The miniplayer resize test reuses one app/context through
wide, narrow and wide sizes; artwork stays grouped while window controls remain
at the outer edge. All six graphic views were inspected natively. Tape motion
and the audio engine are unchanged.

The colored circular-artwork rim was reproduced on the previous local build
with a solid neutral synthetic cover. The same central 200×200-pixel region
contained 140 gold-tinted pixels before the fix and zero afterwards. This is a
fixture comparison, not a general color guarantee for arbitrary artwork.
The fix draws the colored placeholder only when artwork is absent; it also
applies to the CD's circular cover. Native before/after captures were inspected.

## Scope and limitations

Screenshots establish the observed layout and readability, not full
accessibility compliance. The native driver exposes mainly the title bar for
the immediate egui settings viewport; its widget interactions are tested with
egui input events. VoiceOver and complete native keyboard traversal require
separate validation.

The public package is experimental, ad-hoc signed and not notarized, and
excludes the optional LMS runtime. Hardware bit-perfect output, acoustic
gaplessness, Intel macOS and other operating systems are not certified by these
checks. New translations require native-language editorial review. Analog
meters display existing peak telemetry rather than calibrated VU measurements.
See RELEASING.md for distribution status and ArtCraft Launcher requirements.
