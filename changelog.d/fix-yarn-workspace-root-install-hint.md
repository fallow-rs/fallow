### Fixed

- **The suggested Yarn command now works at the root of a Yarn 1 workspace project.** Yarn 1 refuses `yarn add -D <package>` at a workspace root and asks for `-W`. At such a root, Fallow now adds `-W` to these commands:
  - the `fallow doctor` fix commands;
  - the missing-companion hint of `--type-aware`, `type-aware status` and `similar-code`;
  - the missing `fallow-cov` hint;
  - the sidecar install that `fallow coverage setup` runs, and the beacon command that it prints.

  Fallow identifies Yarn 1 from a `packageManager` field with `yarn@1.`, or from the Yarn 1 header in `yarn.lock` when the field does not name Yarn. Yarn Berry rejects `-W`, so its commands do not change. ([#4519](https://github.com/fallow-rs/fallow/issues/4519))
