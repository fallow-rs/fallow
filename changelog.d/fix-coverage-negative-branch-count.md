### Fixed

- **A coverage map with a negative hit count now loads.** Vitest `coverage-v8`
  can write a negative branch count, for example `[5, -1]`, for the implicit
  else of an `if`. Fallow rejected the whole map: `fallow health` and
  `fallow audit` stopped with exit 2 when `--coverage` or `FALLOW_COVERAGE`
  named the map. Fallow now reads a negative count in `s`, `f`, `b` or `b_T`
  as 0, which is a miss. An auto-detected map that fails to load now gives a
  note that says Fallow ignored it, and not a note that says Fallow used it.
  (Closes [#3564](https://github.com/fallow-rs/fallow/issues/3564))
