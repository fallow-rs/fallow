### Fixed

- **`fallow audit` no longer reuses the head analysis as the base when a change can alter the findings.** The audit skips the base analysis when every changed file has the same tokens at base and head. A new or removed `@deprecated` tag or `@jsxImportSource` pragma changes the findings with no token change, so a new `deprecated-export-in-use` finding showed as inherited and the gate passed. A changed `export * as name` also compared as equal, because the tokens did not include the exported name. These changes now run the base analysis.

### Changed

- **The duplication token cache is rebuilt once.** The tokens of `export * as name from '...'` now include `as` and the exported name, so the cache version changes.
