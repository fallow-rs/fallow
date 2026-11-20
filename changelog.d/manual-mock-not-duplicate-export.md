### Fixed

- **`fallow dead-code` no longer reports a manual mock and its module as duplicate exports.** A manual mock at `<dir>/__mocks__/<file>` repeats the exports of the module at `<dir>/<file>` on purpose. Fallow now removes the mock from a `duplicate-exports` group when the group also holds that module. The rule applies to Jest, Vitest and Rstest mocks. A mock that repeats an export of a different file is still reported, and unused exports and unused files in mock files are still reported.
  (Closes [#4855](https://github.com/fallow-rs/fallow/issues/4855))
