### Fixed

- **`fallow dead-code` no longer reports the exports of a Vitest alias replacement file that stands in for a relative import.** An entry such as `{ find: "./clock.js", replacement: "./test/stubs/clock.ts" }` swaps the original file for a stub. Fallow resolves the relative import to the original file, so the stub had no consumer. With `includeEntryExports`, fallow reported the exports of the stub as unused. Fallow now credits every export of a replacement file when the alias key is a relative or absolute path. Thanks @Smrtnyk for the report.
  (Closes [#3963](https://github.com/fallow-rs/fallow/issues/3963))
