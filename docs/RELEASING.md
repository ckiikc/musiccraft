# Release review

The source snapshot is an independent MusicCraft 0.10.37 review candidate.
Public hosting and community-app acceptance are maintainer decisions.

The macOS app currently used for local testing has an ad-hoc signature.
ArtCraft Launcher requires an approved source, release checksums, and the
publisher's Developer ID team for macOS installation. Choose a stable app ID
owned by the eventual publisher. The current review ID is a placeholder.

Run the documented source checks and synthetic FTP fixture before a release.
Publish release archives with SHA256SUMS.txt after testing each advertised
architecture. A local build on Apple silicon does not establish Intel,
Windows or Linux support.

The standard packager excludes the optional LMS runtime. A combined binary
needs a separate review of its exact payload and redistribution obligations.
Keep third-party source licenses and asset notices with any distribution.
