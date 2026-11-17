### Fixed

- **A missing file under a `baseUrl` directory is an unresolved import, not an unlisted package.** With `"baseUrl": "./"`, an import of `src/generated/client` before a code generator runs gave the unlisted dependency `src`. Fallow now reports the unresolved import `src/generated/client` when the first segment of the specifier is a directory under the `baseUrl` of the nearest tsconfig. A package that the nearest `package.json` declares keeps its credit.
  (Closes [#4731](https://github.com/fallow-rs/fallow/issues/4731))
