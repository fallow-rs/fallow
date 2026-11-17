### Fixed

- **A `**` workspace pattern no longer reports source folders as `glob-matched-no-package-json`.** pnpm skips a folder without a `package.json` when it expands `**`, and fallow now does the same. With `packages/**`, a folder such as `packages/a/src` gives no diagnostic, and the run no longer shows as degraded.
  (Closes [#4719](https://github.com/fallow-rs/fallow/issues/4719))
- **A `dir/**` workspace pattern now includes `dir` itself.** pnpm reads `**` as zero or more folders. With `playground/**` and a `playground/package.json`, fallow now analyzes `playground` as a workspace and no longer reports it as `undeclared-workspace`.
  (Closes [#4721](https://github.com/fallow-rs/fallow/issues/4721))
