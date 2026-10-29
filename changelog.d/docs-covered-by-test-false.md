### Changed

- **The fallow skill says that `covered_by_test` can be `false`.** Fallow Cloud now reports `false` when a test or CI run tracked a function and never called it. The MCP reference names the three values, and a hot function with `false` is a high-risk edit, the same as `null`.
