# Direct Android music sharing — protocol 1

MusicCraft Android 0.2.1 and desktop 0.10.49 introduce a direct, read-only local music source. The feature is independent of a developer account, USB debugging, third-party FTP software and personal configuration. It is included in normal builds; ArtCraft distribution does not require a private server or a special activation. Publication in the ArtCraft catalogue remains a separate release process.

## Pairing

1. Put the phone and computer on the same private Wi-Fi network (guest isolation can block connections).
2. On the phone, grant audio access and enable **Condividi con il computer**. Choose **Abbina computer** to show a four-digit code.
3. On the desktop select **Library → Add source… → MusicCraft Android…**. The nearby phones appear automatically. Choose yours and enter its four digits.
4. Approve the request on the phone with **Autorizza**. The desktop stores the resulting credential in macOS Keychain; no address, link or password needs to be copied.
5. Automatic synchronization indexes the current catalogue. The refresh control requests it immediately. Deleted phone tracks disappear only after a complete authenticated catalogue succeeds.

The code is random, valid for two minutes and limited to three exchange attempts. Pairing is available only while its phone dialog is open and still requires explicit phone approval. Closing the dialog cancels the window. Once paired, a computer reconnects without a PIN. Stop sharing from the phone or its persistent notification. To invalidate previously paired computers, stop sharing and choose **Revoca abbinamenti**. Reinstalling/clearing Android data also creates a new identity. No universal password or developer IP is shipped.

The service shares only supported audio already indexed by Android MediaStore. It cannot browse arbitrary directories, upload, rename, remove or edit files. It suspends transfers while MusicCraft intends to play, other music is active or the phone audio mode indicates a call. Pausing/stopping phone audio makes sharing available again. A Mac may finish playing media already present in its own cache; suspending phone sharing does not erase that cache.

Sharing is user-started, with a dedicated connected-device foreground notification. It is not a fake audio playback service and does not start at boot. Phone battery/network restrictions can still stop a background service. Android UI is currently Italian, as in version 0.1.0; desktop pairing and guide text use the existing 23-language system.

## Discovery and security

The phone binds a private IPv4 Wi-Fi/Ethernet address, preferring port 8737 and choosing another free port on collision. An authenticated loopback listener permits isolated USB QA, but USB is not needed for ordinary use. No cellular or wildcard listener is opened. Bonjour/Android NSD advertises `_musiccraft._tcp.` and a device identifier, never credentials. A paired desktop can resolve a changed address without scanning an entire subnet. Initial automatic pairing requires local multicast discovery. If the network blocks Bonjour/NSD, enable local device communication or use another private Wi-Fi network. IPv6-only networks and routed public access are not supported in this first version.

Each installation creates an EC TLS identity in Android Keystore and a random 256-bit access token, encrypted locally with a Keystore AES-GCM key. A short-lived SRP-6a PAKE exchange uses the RFC 5054 3072-bit group and SHA-256, implemented with Bouncy Castle on Android and RustCrypto SRP primitives on desktop. Public values and evidence use Bouncy Castle fixed-width padding. The four-digit code is never sent or hashed into an offline-checkable public password field. After mutual evidence verification and phone approval, AES-256-GCM under SHA-256(PAD(S)) protects the installation identity, SPKI pin and access token. Bootstrap TLS transports PAKE messages only; no music credential is sent through its initially untrusted certificate. The desktop then makes a fresh connection pinned to the authenticated public key. The desktop uses the saved SPKI pin instead of public CA/hostname trust for this private certificate and never follows redirects with this authentication. Discovery addresses alone are never trusted. The desktop checks both the TLS pin and catalogue device identity. Certificate/key changes require pairing again.

Authentication is HTTP Basic over pinned TLS, with username `musiccraft` and the per-installation token. Credentials are not placed in URLs, command-line arguments or logs. Music endpoints accept GET and HEAD only; file write methods return 405, missing/wrong authentication 401, busy phone 503, missing media 404 and invalid/out-of-range byte requests 416. A single explicit/open-ended byte range is supported with 206. Multipart and suffix ranges are rejected. Connections, workers, headers and catalogue sizes are bounded.

Endpoints:

- `/v1/pair/start`, `/v1/pair/finish`: bounded POST requests for a user-opened SRP pairing window; no file access, code limit and explicit approval.

- `/v1/status`: authenticated protocol/device identity and sharing availability.
- `/v1/catalog`: complete protocol/device identity plus supported tracks (MediaStore ID, virtual filename, title, artist, album, duration, number/disc, size, modification time).
- `/v1/audio/ID.EXT`: read-only MediaStore audio; full body or byte range, with Content-Length and Accept-Ranges.

The initial protocol shares embedded artwork through the audio file; separate booklet/folder images and file writes remain outside the protocol.

## Reproducible checks

Android: `./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:testDebugUnitTest :app:lintDebug`. The custom emulator-only instrumentation verifies HTTPS identity persistence, authentication, read-only methods, full/partial byte equality, HEAD, invalid ranges and suspension/resumption around real Media3 playback.

Desktop: `cargo test --locked -j 2`, `cargo clippy --locked -j 2 --all-targets -- -D warnings`, `cargo fmt -- --check`. An ignored cross-platform test uses private fixture identity/code files (`MUSICCRAFT_QA_PAIRING`) and the emulator-only synthetic server. It verifies refusal of a wrong PIN, SRP agreement, approval-gated encrypted identity transfer, complete catalogue/sync, scoped removal, no removal after pin failure and exact decoded PCM for three FLAC fixtures. It rejects non-loopback targets and never uses a personal library.

Builds and debug APKs contain code/assets only. Do not include local.properties, signing keys, Android/desktop profiles, private pairing links, raw device logs, screenshots or test outputs in published packages. The debug signing certificate is for local testing; release distribution needs its own maintained signing process.
