### Fixed

- **`fallow audit` no longer reuses a cached base result of another
  type-aware selection.** The base snapshot cache key now holds the
  effective selection from `--type-aware`, `--no-type-aware`,
  `FALLOW_TYPE_AWARE`, `audit.typeAware` and `typeAware`.
- **`fallow audit --coverage <dir>` no longer reuses a base score from
  old V8 coverage dumps.** The cache key now holds each coverage file
  that the base pass reads.

### Performance

- **A change of a saved-baseline path keeps the cached base result of
  `fallow audit`.** The base pass reads no saved baseline, so the paths
  are not part of the cache key.
