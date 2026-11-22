### Fixed

- **`fallow audit` no longer reuses the head analysis as the base when a change can alter the findings.** The audit skips the base analysis when every changed file has the same tokens at base and head. A new or removed `@deprecated` tag or `@jsxImportSource` pragma changes the findings with no token change, so a new `deprecated-export-in-use` finding showed as inherited and the gate passed. A JSDoc `@import` tag, a `@jsxRuntime` pragma, a triple-slash reference and a `fallow-override-threshold` comment had the same problem. A changed `export * as name`, a default import against a namespace import, and a type-only import or export against a value one also compared as equal, because the tokens did not include the name or the form. These changes now run the base analysis.

### Changed

- **The duplication token cache is rebuilt once.** The tokens of `export * as name from '...'`, of a namespace import and of a type-only import or export now include the name or the `type` keyword, so the cache version changes. A clone group that holds one of these statements can change once.
