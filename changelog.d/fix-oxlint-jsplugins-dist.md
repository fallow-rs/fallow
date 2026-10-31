### Fixed

- **`fallow dead-code` no longer reports the default export of a local Oxlint `jsPlugins` directory whose `main` field points at build output.** A workspace plugin package can set `main` to a built file, for example `"./dist/src/index.js"`. Fallow now maps that file back to its source file, for example `src/index.ts`, with the same rules as for a package entry. The `rootDir` and `outDir` options of `tsconfig.json` apply. Other exports of the source file stay reportable. Thanks to @Smrtnyk for the report.
  (Closes [#4034](https://github.com/fallow-rs/fallow/issues/4034))
