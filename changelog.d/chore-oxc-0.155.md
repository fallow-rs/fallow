### Changed

- **Fallow now parses with Oxc 0.155.** A short TypeScript input that made the parser use seconds of time and gigabytes of memory (oxc-project/oxc#27637) now stops at its syntax error at once. Analysis output for real projects does not change.
