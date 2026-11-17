### Changed

- **PGO Validate skips pull requests that cannot change a PGO build.** The workflow starts for each edit of `Cargo.toml` or `release.yml`, and its comparison takes more than an hour on a macOS runner. A version bump, a dependency entry, a lint or a publish job change now skips the comparison. A change of a release profile section, of the top-level `env` of `release.yml`, or of the release jobs that train and use the profile, still runs it. The weekly run on `main` still covers dependency changes. This change affects contributors only.
