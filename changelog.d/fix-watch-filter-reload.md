### Fixed

- **`fallow watch` applies a changed `ignorePatterns` or `.gitignore` to the events it watches.**
  Before, a config change reloaded the analysis config but kept the old event
  filter. A path that the new config ignores still started a new analysis, and
  a path that the old config ignored did not. The filter now follows each
  reload.
