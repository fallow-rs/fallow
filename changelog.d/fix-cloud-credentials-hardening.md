### Fixed

- **`fallow inspect --cloud` no longer waits for a locked keychain.** The
  keychain read (`security` on macOS, `secret-tool` on Linux) now has a time
  limit of 1 second. At the limit, Fallow stops the tool, reads the
  credentials file, and reports `no_key` when no key is stored. In
  `fallow cloud login` and `fallow cloud logout`, each keychain call has a
  limit of 60 seconds, so a person can answer an unlock prompt.
  (Closes [#4003](https://github.com/fallow-rs/fallow/issues/4003))
- **`fallow cloud login` and `logout` no longer lose keys in the credentials
  file.** A change to the file now holds an advisory lock on
  `credentials.lock` in the same directory, so two logins at the same time
  for two API URLs keep both keys. When the file does not parse, the command
  stops with a message that names the file and does not change the file.
  (Closes [#4004](https://github.com/fallow-rs/fallow/issues/4004))
