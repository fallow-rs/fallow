### Fixed

- **`fallow dead-code --include-entry-exports` no longer reports the export
  that a Serverless handler names.** A handler such as `src/users.main` makes
  `src/users.ts` an entry point, and the framework calls its `main` export.
  The Serverless Framework and osls plugins now mark that export as used.
  Other exports of the handler file still report. Output without
  `--include-entry-exports` does not change.
