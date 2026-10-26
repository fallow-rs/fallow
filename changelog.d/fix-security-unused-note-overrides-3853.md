### Fixed

- **`fallow security` follows per-path overrides for its dead-code note.** A sink in a file where an override sets `unused-files` or `unused-exports` to `off` no longer shows the "also reported as unused-file" or "unused-export" note, and no longer gets the delete or remove action. Thanks to [@Smrtnyk](https://github.com/Smrtnyk) for the report.
  (Closes [#3853](https://github.com/fallow-rs/fallow/issues/3853))
