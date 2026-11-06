### Fixed

- **`fallow dead-code` reads the tool that a Yarn `run` call starts.** A
  package script such as `run -T jest --config ./jest.config.front.js` or
  `yarn run -T jest ...` now counts as a `jest` call. The Jest plugin then
  reads the config file and its setup files. Yarn 2 and later puts the `run`
  command on the `PATH` of each package script. Fallow now also skips the
  `yarn run` flags `-T`, `-B`, `--inspect`, `--inspect-brk` and `--require`.
- **`fallow dead-code` no longer reports a Jest `moduleNameMapper` target as
  an unused file.** A target that names one file with `<rootDir>`, such as
  `'<rootDir>/src/test/mocks/inquirer.js'`, is now an entry point. A target
  with a `$1` capture group or without `<rootDir>` adds no entry point.
- **`fallow dead-code` finds the scripts that the changesets GitHub Action
  runs.** The `version` and `publish` inputs of `changesets/action` are shell
  commands, so a file that such an input runs, for example
  `node .github/changeset-version.js`, is now an entry point. The `cwd` input
  sets the directory of these commands. The inputs of other actions add no
  entry points.
