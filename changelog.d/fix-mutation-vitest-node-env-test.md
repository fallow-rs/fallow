### Fixed

- **`fallow mutation` now runs Vitest tests under `NODE_ENV=test`.** The
  Vitest adapter sets `TEST=true`, `VITEST=true` and `NODE_ENV=test`, as the
  `vitest` command does. Before, Vite set `NODE_ENV=development`, so code that
  validates its environment unless `NODE_ENV` is `test` gave baseline
  failures. A `NODE_ENV` value of the environment stays.
