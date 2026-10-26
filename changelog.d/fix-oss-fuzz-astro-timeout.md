### Fixed

- **Astro extraction avoids repeated scans of malformed script and style tags.** Quoted attributes retain their original source offsets and Unicode tag matching while incomplete tags no longer cause quadratic region scanning.
