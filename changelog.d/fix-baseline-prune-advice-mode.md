### Fixed

- When a health or duplication baseline becomes stale, the prune command in the warning and gate failure now includes `--production` if the baseline was saved in production mode. Prune advice also includes `--root` when the project is outside the working directory.
