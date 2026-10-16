### Fixed

- **An `import type` in a test no longer makes a file test-reachable.** The
  build erases a type-only import before the test runs, so the test does not
  run the file or the modules that the file imports. Test reach now skips an
  import edge when every symbol on it is type-only. Coverage gaps and the CRAP
  estimate report these files as untested. A file that declares only types has
  no runtime code, so coverage gaps do not count it as a runtime file and
  never report it as untested. Dead-code reachability does not change.
  (Closes [#3400](https://github.com/fallow-rs/fallow/issues/3400))
