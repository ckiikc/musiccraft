# MusicCraft 0.11.0 desktop preview

This branch ports the public desktop application to Windows x86-64 and Linux x86-64. macOS retains its existing bundle identity, profile and CoreAudio implementation. The Android project is independent and unchanged.

## Platform boundaries

| Function | macOS | Windows | Linux |
|---|---|---|---|
| Persistent profile | `~/Library/Application Support/MusicCraft` | `%LOCALAPPDATA%\MusicCraft` | `$XDG_DATA_HOME/musiccraft`, otherwise `~/.local/share/musiccraft` |
| Saved passwords/pairing | existing Keychain service | Windows Credential Manager | desktop Secret Service over D-Bus |
| Paired phone discovery | existing native DNS-SD | in-process mDNS | in-process mDNS |
| Shared audio | CoreAudio | WASAPI | ALSA, normally through desktop sound server |
| Exclusive PCM/DoP | existing CoreAudio HAL | native WASAPI exclusive | direct ALSA `hw:` device |
| File-browser open | existing Open Documents | per-profile authenticated loopback IPC | per-profile authenticated loopback IPC |
| Installer | existing app packager | per-user NSIS, portable ZIP | Debian package, tarball with per-user installer |

Secrets are never written to the profile or a plaintext fallback. If the desktop credential store is locked or absent, saving fails with a visible error and the credential remains in memory for that session. Pair each OS installation independently; do not copy another OS's credential store. Updates preserve the profile and installer removal leaves it intact.

Windows requires Windows 10/11 x86-64, system curl and graphics support compatible with eframe/Glow. Windows ARM64 and 32-bit Windows are not native targets of this preview. Linux packages target Debian 12/Ubuntu 24.04 or newer; the tarball retains the same system-library requirements. X11 work areas and Windows monitor work areas are queried natively. Windows/Linux use an opaque miniplayer surface; Windows retains its native window frame for consistent client-area sizing. The macOS transparent surface is unchanged. Wayland deliberately leaves placement, stacking and always-on-top behavior to its compositor. A D-Bus desktop session, Secret Service provider and an XDG file-dialog portal/backend are required for the corresponding integrations.

## Audio formats

The existing Symphonia path remains primary for FLAC, ALAC, WAV/AIFF, MP3, AAC and Vorbis. A separate bounded subprocess lane adds APE, WavPack, WMA, Opus and DSD-to-PCM decoding using unmodified minimal LGPL FFmpeg 8.0.1. Each installer includes that runtime, its complete source archive, license and build recipe. The optional LMS runtime remains excluded; external LMS control remains available.

Additional-codec subprocesses receive only local file/cache paths, never network credentials or media URLs. They have no network protocols, bounded PCM queues, cancellation and seek through the existing player worker. Original remote files for these extra codecs are authenticated and cached before decoding; the current limits are 1 GiB per file and 2 GiB for that cache. DSF/uncompressed DFF in DoP mode instead use progressive, seekable authenticated reads. No listening-time limit is added.

Encrypted/DRM WMA is unsupported. WavPack hybrid correction `.wvc` files are not applied; the base file can play and strict integer rejects hybrid data. Headerless raw DSD has no implicit rate/channel layout: supported DSD containers are DSF and uncompressed DSDIFF, including renamed files with recognizable headers. Compressed DST can use PCM conversion, not the direct DoP reader.

## DSD / DoP

In Audio settings choose **DSD output → DoP** only for a compatible DSD DAC, then choose exclusive output and its hardware device. Default remains PCM conversion. DoP carries original DSD bits in integer frames with alternating `0x05`/`0xFA` markers. DSD64 requires 176.4 kHz transport, DSD128 352.8 kHz, and higher rates require corresponding device support. There is no fallback to a shared mixer, float, software gain, ReplayGain or resampling in this path. The endpoint's hardware volume must be verifiable at unity and unmuted; unsupported configurations fail explicitly without changing system volume.

The renderer generates markers continuously across compatible track boundaries and emits marked DSD silence during buffer starvation without advancing the audio position. PCM peak meters are not measurements of DSD and remain inactive in DoP. A format/transport change reopens the device rather than mixing PCM and DoP. Raw ASIO/native USB DSD is a separate, unimplemented transport.

Synthetic byte tests establish bit/channel ordering, seek, bounds and renderer framing. They do not certify a physical DAC, its driver, an acoustic transition or hardware bit-perfect output. DoP hardware validation must use a compatible DAC, never an ordinary speaker output.

Specifications: [DoP 1.1](https://dsd-guide.com/dop-open-standard), [DSF specification](https://dsd-guide.com/technical-documents). Runtime source/license: [FFmpeg](https://ffmpeg.org/legal.html).

## Reproduce builds

Use Rust 1.97.1 and at most two Cargo jobs. All runs use synthetic files and `--data-dir` isolated from normal profiles.

The supplied `packaging/Dockerfile` is a pinned x86-64 Debian build/QA environment. It includes test-only tools such as Wine and a virtual display; these are not packaged with the app. Build and package from its source mount with a separate Cargo target volume:

```sh
scripts/build-codec-runtime.sh linux
scripts/build-codec-runtime.sh windows
scripts/build-desktop.sh x86_64-unknown-linux-gnu
python3 scripts/package-desktop.py linux --target x86_64-unknown-linux-gnu
scripts/build-desktop.sh x86_64-pc-windows-gnu
python3 scripts/package-desktop.py windows --target x86_64-pc-windows-gnu
```

Windows cross-builds require the MinGW-w64 toolchain and NSIS. Native Windows source checks use MSVC and the Windows SDK. Artifacts and checksums are generated under ignored `dist/`; these commands neither install nor publish. The per-user Windows installer registers Open With/Default Apps capabilities and never overwrites the user's default handler. Linux registers a desktop entry and supported MIME types without changing defaults.

For macOS extra formats, run `scripts/build-codec-runtime.sh macos` before packaging. The macOS packager includes that matching runtime if available, while retaining the established public application/profile identity. A missing runtime produces an explicit error for extra codecs, not silent omission or an automatic download.

## Validation boundaries

See `VALIDATION.md` for checks actually performed on this branch. Real Windows, multi-monitor configurations, Wayland compositor policies and physical exclusive/DoP DAC behavior require their own validation. Source CI and package generation do not establish those results automatically.
