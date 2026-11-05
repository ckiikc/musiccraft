# MusicCraft contributor instructions

Read README.md, CONTRIBUTING.md, THIRD_PARTY.md and docs/ARCHITECTURE.md first.
MusicCraft is independent; community-app consideration does not imply ArtCraft affiliation.

- Use a focused codex/ branch and at most two Cargo jobs. Preserve public app identity and profiles.
- Use synthetic media and isolated --data-dir settings. Keep personal media, credentials,
  saved libraries, downloaded metadata and local diagnostic outputs out of Git and releases.
- Run fmt, Clippy, ordinary tests, the publication scan and relevant synthetic integrations.
  Ignored tests require explicit prerequisites; never run a private-library audit by default.
- The standard package excludes LMS. Keep third-party notices with binary distributions.
- Do not infer hardware bit-perfect output or other-platform support from unit tests.
- Publishing, installation and external messages require user authorization.
- Freebuff and other delegated agents are opt-in per task.
