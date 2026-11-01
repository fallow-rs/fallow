### Fixed

- **A pnpm override with a collection value is now reported as misconfigured,
  and a deep one no longer exhausts memory.** pnpm reads an override value
  as a string. Fallow wrote a mapping or list value back as YAML text, which
  grew with the square of the nesting depth, and accepted a mapping with one
  entry as a version. Fallow now treats a collection value in
  `pnpm-workspace.yaml` as a missing value, as it already did for npm
  `overrides` and bun `resolutions`. Found by OSS-Fuzz.
