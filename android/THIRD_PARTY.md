# Android dependencies

Media3/ExoPlayer 1.11.1, AndroidX (including Car App 1.7.0), Kotlin standard library, Guava and JSpecify: Apache License 2.0. Sources: https://github.com/androidx/media, https://android.googlesource.com/platform/frameworks/support/ and https://github.com/jspecify/jspecify.

Bouncy Castle `bcprov-jdk18on` 1.85.2 (resolved by SMBJ; direct minimum 1.83): Bouncy Castle licence (MIT-style), https://www.bouncycastle.org/licence.html. Used for SRP-6a and the SMBJ cryptographic provider; no global security provider is installed or replaced. The distributed JAR/APK retains upstream licence metadata.

Source project: https://github.com/bcgit/bc-java. No code, credentials or visual assets from other Craft applications are bundled.

The rating metadata JNI library is built from this repository's MIT-licensed `musiccraft-tags` engine. Its Rust dependencies include jni-rs 0.21.1 (MIT OR Apache-2.0), Serde/serde_json (MIT OR Apache-2.0), and RustCrypto SHA-2 (MIT OR Apache-2.0), plus their Cargo.lock-pinned transitive dependencies. No third-party audio/tag writer is used; supported tag fields are patched without decoding or transcoding audio. Sources: https://github.com/jni-rs/jni-rs, https://github.com/serde-rs/serde, https://github.com/serde-rs/json, https://github.com/RustCrypto/hashes.

## Direct NAS synchronization

SMBJ 0.15.0 and ASN.One 0.6.0 are Apache-2.0 (https://github.com/hierynomus/smbj, https://github.com/hierynomus/asn-one). SLF4J 2.0.18 and MBassador 1.3.2 are MIT. Exact upstream texts are retained in APK assets alongside the Bouncy Castle license. The shared MIT synchronization planner is under `crates/musiccraft-sync`, exposed through the existing JNI library; its lockfile-pinned Rust dependency notices are generated into `native-licenses.txt`. No global security provider is installed.
