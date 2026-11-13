# Desktop 0.10.57 source update — 2026-11-10

The development source passed 218 ordinary tests, with 42 prerequisite-based tests excluded, Clippy for all targets with warnings denied, and a release build. Regression coverage includes interrupted reads, DNS expiry across audio ranges, authenticated phone reconnection, album membership in a flat catalog, title-text interactions, the first volume gesture across activation, current-track selection and stereo row-indicator geometry. The first fresh-launch volume gesture was also confirmed by the operator in an isolated native QA build.

On the installed development build, native screenshots of real playback showed different L/R bar heights over consecutive frames. This is a UI/telemetry observation, not an acoustic capture or a hardware audio certification. Native visual fixtures use invented metadata; private screenshots and profiles are excluded from Git.

The public export keeps the established public bundle ID, Keychain service and profile location. Personal network literals in synthetic tests are replaced with generic fixture addresses, and credential-rejection fixtures use explicit dummy values. The publication scanner reported no findings before commit preparation. At the user's request, no additional local test suite was run on this sanitized export; CI may run its normal source checks after publication. Android inputs and the established public packager remain unchanged.

The source update is not a new public binary release. Existing downloadable releases retain their own versions and checksums.

---

# MusicCraft desktop 0.10.50 and Android 0.3.0 — 2026-11-08

The sanitized public desktop source passed 198 ordinary tests (40 prerequisite-based tests excluded), formatting, Clippy all-targets with warnings denied, all 12 authenticated loopback FTP integrations, three Mach-O identity tests, one document-association test and publication scanning. Tests use synthetic files and a fresh profile. The source/package retain the stable public app ID and do not import the private development profile.

Desktop regression tests cover the order delivered by file browsers, sequential next-track behavior independent of prior shuffle/repeat-one settings, URL decoding and supported document associations. The Android phone source adds pinned HTTPS catalog/streaming and four-digit, approval-gated SRP pairing. The earlier interoperability test against an emulator verified bad PIN rejection, approval-gated identity transfer, catalog/sync and decoded fixture PCM. That interoperability run predates the public snapshot; the loopback-only ignored integration remains available in source and has not been rerun for this publication.

The Android project in `android/` was built from this public tree: debug app and test APKs, eleven unit tests and lint (zero errors, 22 warnings). Its application payload matches the previously installed/tested 0.3.0, apart from stripping ancillary PNG icon metadata; the unit-test search example uses synthetic names. The ordinary APK contains no test music or per-device credentials. Android native tests previously passed in a dedicated emulator: independent search/queue; full Player with live portrait/landscape recreation; queue edits; persistent playlists, rules and ratings; dynamic replenishment and bounded queue; stop-adding preserving the queue; force-stop restoration in pause. See [Android validation](../android/VALIDAZIONE.md) for the exact tested layers, early timing failures and hardware limits.

No new autoradio/DHU test, physical audio capture, external-DAC certification, Intel build or Windows/Linux validation is claimed. Android Auto is a media-service integration; the sideloaded APK is not a Play Store release. Android is currently Italian only; desktop interface/help has 23 languages with native editorial review pending. Neither app imports LMS statistics or duplicates every LMS dynamic-playlist preset.

## About panel (0.10.50)

The default AppKit About action used normal window level zero, below the pinned player (level three). The application now wraps only that menu action, retains the standard Apple panel, recalculates its level from visible normal/pinned application windows and orders it above the player. Transient menus/tooltips and the cached About panel are excluded from the owner level; the saved player preference is not changed. A newly created panel is preferred over a previously closed instance.

The debug-only, explicitly isolated native preview invokes the installed menu action, closes the panel and reopens it. Native window inspection and a captured version panel confirm level four above the pinned player's three; normal mode uses one above zero. The panel shows 0.10.50. The unit regression and ordinary suite pass; these checks concern window ordering, not audio changes.

## Earlier desktop validation (0.10.44)

The separate public checkout passed 192 ordinary tests (39 prerequisite-based
integrations excluded), all 12 synthetic authenticated loopback FTP integrations,
three Mach-O identity tests, formatting, Clippy all-targets with warnings denied,
and publication scanning. The package passed deep signature verification and a
native smoke test using an empty isolated profile. Its version, public identity,
absence of LMS runtime and absence of private build paths were checked.

## Window-drag fix

The previous path waited for egui's drag threshold, then sent `StartDrag` to
winit 0.30.13. Its macOS implementation uses `NSApplication.currentEvent()`;
AppKit requires the original mouse-down. Native diagnostic traces during real
mouse drags showed the current event was type 6 (LeftMouseDragged), while the
corrected handler supplied the retained type 1 (LeftMouseDown) event.

The correction retains the original application-local mouse-down and window,
binds it to the UI press using window size and pointer coordinates, and checks
that the event and window frame still match before calling AppKit. It passes
events through unmodified and removes its local monitor at exit. Release,
pointer loss, focus loss and full/mini geometry changes cancel the gesture.
There is no fallback to a stale current event. The audio engine is unchanged.

The interrupted-gesture regression test fails on the previous handler: pointer
movement after interruption can restart a native drag without a fresh press.
It passes with the correction. Existing click, jitter, stationary long-press,
secondary-button and one-start-per-gesture tests remain in the suite.

A separate native QA build used 72 synthetic tracks and no audio output.
The library-to-mini transition was driven automatically; the user performed
real mouse drags and repeated full/mini transitions and confirmed no jumps.
Three native drag starts supplied the retained original event while the current
event was already LeftMouseDragged. Background driver drags did not initiate a
real native drag and are not counted as successful interaction tests.

Debug tracing requires an explicit `--data-dir` and `--window-drag-trace`.
It records event identifiers and geometry, not library text or credentials.
Private raw traces, desktop coordinates and personal screenshots are excluded
from this repository. The standard release binary excludes debug tracing.

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

## 0.10.62 / Android 0.4.1

Development validation: 245 macOS tests (43 explicit hardware/integration tests ignored), formatting and Clippy; 4 shared-engine tests; 25 Android JVM tests, Lint and API-33 emulator instrumentation. Native isolated star edits were checked with independent ffprobe and byte-identical decoded PCM. FLAC/MP3 payload preservation, recovery, request authorization, numeric rule serialization and explicit vote precedence were checked in the emulator. The real desktop HTTPS client completed a rating edit after app/system approval. Native 500-row library and queue fixtures and ReFacets-open/closed virtualized layout tests verified following. Private raw evidence is not distributed. Physical-device installation validation is reported in release notes after completion. No hardware bit-perfect or Android Auto driving claim follows from these tests.

## 0.10.63 / Android 0.4.2

Development build: 250 desktop tests passed (44 integrations/hardware checks explicitly ignored), formatting and Clippy; shared sync 9 tests and metadata engine 4 tests. Android JVM 25 tests and Lint passed. API33 emulator tests verified real signed loopback SMB and SAF copies, conflicts, bidirectional baseline/deletion recovery, exact bytes, stale previews, stable playlist/stat identities and advancing sync versions. The desktop HTTPS client completed upload, download and recoverable deletion with approval of both exact previews on the emulator. Rating-tag preservation/recovery regression passed. Native macOS screenshots verified the compact Library-only selectors. These results do not certify the user's physical NAS, a real car or hardware audio output. Public-layout build checks are recorded separately in release notes. Private diagnostic outputs are excluded.
