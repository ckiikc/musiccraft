# Security and privacy

This alpha has not received an independent security audit. Report a security issue privately through the repository's GitHub security advisory channel when available. Do not put credentials, source URLs with passwords, library inventories or identifying screenshots in a public issue.

Remembered FTP passwords use macOS Keychain and are excluded from source configuration serialization. Plain FTP does not encrypt them in transit. Optional device rediscovery uses the configured port on bounded connected private networks; it verifies the library before rebasing a source and rejects ambiguous matches.

The optional media server shares only its published selection with tokenized URLs and bounded requests. Its HTTP transport is not Internet-facing authentication or encryption. Keep it on a private network; do not forward its port. URLs and tokens should not be posted in issues. Integrated LMS uses its own private profile and randomized internal credentials when the optional local runtime is available.

Online metadata providers receive search terms when the respective features are enabled. Saving a missing cover is a separate optional write to the album directory; it must not overwrite existing artwork.

For diagnostics, use a synthetic reproduction. Redact usernames, network addresses, full paths and tokens from logs. The local publication check is a useful filter, not a complete secret scanner or security certification.


The separate Android music server defaults to read-only HTTPS with a per-installation Keystore identity. It advertises on the local Wi-Fi and requires short-lived, attempt-limited SRP pairing plus explicit phone approval. Desktop reads use the pinned phone key and credentials stored in Keychain. Sharing pauses during local phone playback. Neither codes nor installed credentials are in source or release packages. Revoke pairings on the phone if a paired computer is no longer trusted. See docs/SHARING.md for protocol details and limits.

## Optional reviewed file synchronization

Version 0.4.2 adds a separate, opt-in writable folder grant. A paired computer must submit an exact bounded plan, the phone must approve its preview, and each operation verifies content preconditions. This permission is independent from rating-tag approval and ordinary read-only sharing. Revoking the folder/computer permission blocks further commits. Replacement/deletion retains originals under a hidden recovery directory; grant access only to a dedicated music root and keep recovery space available. Signed SMB2/3 credentials are held for the current direct-sync screen, not persisted. This feature has synthetic integration tests, not an independent security audit or validation against every NAS/provider. See docs/SYNCHRONIZATION.md.
