### Fixed

- **`fallow trace --path` shows the source line of a re-export.** A hop through a barrel file previously pointed to line 1 even when the file declared the re-export further down.
