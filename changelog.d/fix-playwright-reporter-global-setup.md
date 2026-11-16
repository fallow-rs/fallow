### Fixed

- **The Playwright plugin reads a `reporter`, `globalSetup` or `globalTeardown` value that a condition picks.** A config such as `globalSetup: external ? undefined : './setup.ts'` made `fallow dead-code` report the setup and reporter files as unused. The plugin now reads each branch of `a ? x : y`, and the operands of `a || x` and `a ?? x`. A `globalSetup` array also gives each of its files. A template literal without expressions, such as `` `./setup.ts` ``, still counts.
  (Closes [#4620](https://github.com/fallow-rs/fallow/issues/4620))
- **With `includeEntryExports`, the default export of a Playwright reporter, `globalSetup` or `globalTeardown` file counts as used.** Playwright loads these files from the config and reads their default export. The credit also covers the TypeScript source of an ESM specifier, such as `reporter.ts` for `./reporter.js`. Other exports of these files stay reported.
  (Closes [#4621](https://github.com/fallow-rs/fallow/issues/4621))
