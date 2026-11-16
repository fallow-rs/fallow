### Fixed

- **`constant-parameter` follows a loaded value through `??`, `||` and a ternary.** Code such as `const api = mod.default ?? mod; api.migrate()` on a value from `import(path)` was an escape, so the rule skipped each exported function of the project. The rule now reads the result of the fallback when each branch is the loaded value or one member of it, and skips only the exported functions with a name that the code reads. An `await` of the loaded value also keeps it. A branch with any other value is still an escape. The extraction cache version goes up, so the first run after the update parses each file again.
  (Closes [#4594](https://github.com/fallow-rs/fallow/issues/4594))
