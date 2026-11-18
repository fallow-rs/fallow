### Fixed

- **An import of `./x.ts` resolves to `x.d.ts` when `x.ts` does not exist.** TypeScript resolves the import this way. Fallow reported the import as unresolved and reported `x.d.ts` as an unused file. A file `x.ts` still takes precedence over `x.d.ts`.
  (Closes [#4734](https://github.com/fallow-rs/fallow/issues/4734))
