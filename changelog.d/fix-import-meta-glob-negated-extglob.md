### Fixed

- **`import.meta.glob` patterns with extglob groups now match files.** Vite
  matches these patterns with picomatch, but fallow read a group such as
  `!(*.spec)` as literal text. A pattern such as `./**/!(*.spec).{j,t}s` then
  matched no file, and fallow reported each file that the pattern loads as
  unused. Fallow now supports the groups `@(..)`, `?(..)`, `+(..)`, `*(..)` and
  `!(..)`. A negated group excludes the files that match its alternatives. A
  dynamic import pattern that is not a valid glob now gives a warning.
  Thanks [@jleifeld](https://github.com/jleifeld) for the report.
  (Closes [#3489](https://github.com/fallow-rs/fallow/issues/3489))
