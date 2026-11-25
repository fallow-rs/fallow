### Fixed

- **Fallow warns when the parse cache is too small for the project.** When
  the files of a run do not fit in the parse cache cap, each run parses a
  part of an unchanged tree again, and the cache looked broken. Fallow now
  prints one warning for each run. The warning names the share of files that
  do not fit and a `cache.maxSizeMb` or `FALLOW_CACHE_MAX_SIZE` value that
  holds them. `--quiet` hides the warning, and JSON and SARIF output on
  stdout do not change. (Closes [#5216](https://github.com/fallow-rs/fallow/issues/5216))
