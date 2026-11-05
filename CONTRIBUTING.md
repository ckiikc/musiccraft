# Contributing

MusicCraft is an independent macOS alpha. Discuss large changes before implementing them; keep a fix focused and describe its observable behavior and validation.

Use Rust 1.97.1, run the checks in the README, and use at most two Cargo jobs for local work. Test with synthetic audio and isolated `--data-dir` profiles. Never commit a real library, credentials, Keychain exports, downloaded covers/lyrics, personal screenshots or raw diagnostic reports. A private-library audit needs the library owner's explicit consent.

Audio output changes need meaningful PCM/format tests. Synthetic tests do not certify a physical DAC, acoustic gaplessness, or multiroom synchronization. Describe those limits explicitly. UI fixes should cover the relevant input sequence and inspect the real app with a separate profile. Preserve the user's active app and playback during development.

Choose a stable publisher-owned app ID before a public release. Subsequent changes need a tested migration. Do not combine binary redistribution of LMS with the MIT source package unless the exact payload, corresponding sources and notices have been reviewed. The standard packager intentionally excludes that runtime.

Preserve third-party attribution and document every new asset. Do not copy source, icons or screenshots from foobar2000 or other products. The faceted navigation and miniplayer are independent implementations of familiar interaction patterns.

Credit contributions through a public identity chosen by each contributor. Use a GitHub noreply address for public commits.
