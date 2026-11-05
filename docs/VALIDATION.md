# Source review status

This independent MusicCraft 0.10.37 snapshot was reviewed on 2026-11-05.

Completed on the sanitized source tree on Apple silicon macOS:

- 170 ordinary tests passed. The suite keeps 38 prerequisite-dependent tests ignored.
- All 11 authenticated loopback FTP integration tests passed with synthetic files, including delayed and truncated progressive audio ranges.
- Formatting and Clippy passed with warnings denied.
- The targeted source publication scan found no findings.
- The separate review for identifying strings, saved configuration files, private media and PNG metadata found no findings.
- A fresh default macOS package built and passed deep code-signature verification. Its signature is ad-hoc. The package excludes the LMS runtime.
- The dependency inventory contains 247 packages, each with a declared license expression.

The installed local application also passed a language persistence check with version 0.10.36. Vietnamese survived a complete close and reopen. Selecting Italian changed the interface and guide back to Italian.

This snapshot contains source files and required original or third-party assets.
Build outputs and profile data are kept outside the source archive.

The checks are local. This snapshot has no completed public CI run and has not received an independent security review or native-language editorial review. Hardware output, Intel macOS and other operating systems require separate validation. The dependency inventory records declared licenses and does not certify redistribution of a combined binary.


A pointer regression test first reproduced text selection swallowing facet-row
clicks. The 0.10.37 fix passed clicks on text and row padding in the Artist,
Album and Year columns. The installed 0.10.37 app also passed native row
selection and filtering with synthetic tracks.
