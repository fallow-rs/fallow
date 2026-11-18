### Fixed

- **`fallow-report-protocol` no longer pulls in `chrono`.** Its optional
  `schemars` dependency used the workspace entry, which turns on the
  `chrono04` feature. Each consumer of the `schema` feature then got
  `chrono`, and the unmaintained advisory RUSTSEC-2026-0543 with it. No type
  of the crate uses `chrono`.
