### Fixed

- **`fallow dead-code` no longer reports a type that a held binding gives to the inferred type of a used export.** In `const entryBox: Box<Entry> = ...; export const status = { latency: entryBox };`, the inferred type of `status` contains `Box<Entry>`. A declaration file of a module that holds `status.latency` must import `Box` and `Entry`, so the two types are not unused while `status` has a use. Fallow follows object property values, array elements, conditional and logical branches, `satisfies` and `as const`. It does not follow a member read, a call, or a spread. The extraction cache version changes.
  (Refs [#4850](https://github.com/fallow-rs/fallow/issues/4850))
