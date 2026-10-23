### Added

- **Fallow reports config entries that match something but change nothing in the dead-code run.** Three new kinds in `workspace_diagnostics[].kind` carry these entries, each with the config entry in `pattern`:
  - `entry-pattern-redundant`: an `entry` pattern whose matched files are all runtime entry points from another source, for example `package.json` fields and scripts, a plugin, an infrastructure config or `dynamicallyLoaded`.
  - `ignore-dependencies-entry-redundant`: an `ignoreDependencies` entry that hid no dependency finding. A glob that matches no declared dependency keeps the `ignore-dependencies-glob-unmatched` kind and is not reported twice.
  - `ignore-patterns-entry-redundant`: an `ignorePatterns` glob that excluded no file that a built-in ignore, `.gitignore` or the file type filter does not already skip. A glob for build output has an effect only after a build, and the message says so.

  The entries reach JSON, SARIF `toolConfigurationNotifications[]`, the stderr note in human, compact, CodeClimate and `github-annotations`, and the LSP information diagnostic on the config entry. Markdown, `github-summary`, the PR comments and the review summary add a separate `Redundant config entries` section after `Unmatched config patterns`. A `--production` run reports none of the three kinds, because that run checks fewer files and dependency sections. No exit code changes, and no `schema_version` changes, because `workspace_diagnostics[].kind` is an open set.
  (Closes [#3824](https://github.com/fallow-rs/fallow/issues/3824))
