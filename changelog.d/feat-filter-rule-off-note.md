### Added

- **An explicit flag now says when its rule is off in config.** Before, `fallow dead-code --unused-deps` with `unused-dependencies` at `off` printed `No issues found`. Nothing said that the check did not run. Now Fallow prints `Note: unused-dependencies is off in config, so --unused-deps did not check it. Set the rule to "error" or "warn" to run this check.` on stderr.
  - JSON has a new open-set `workspace_diagnostics` kind, `requested-rule-off`, with `rule` and `flag`.
  - SARIF has a configuration notification, and the Markdown formats have a section.
  - MCP adds a root `warnings` sentence. The GitHub Action and the GitLab template print a job-log warning.
  - The note fires for each rule that an explicit selection asks for, on `dead-code` and on `architecture --cycles`, `--boundaries` and `--policy`.
  - The note does not fire for a run with no selection. It also does not fire for a flag that turns its rule on (`--private-type-leaks`, `--absent-component-props`, `--deprecated-exports-in-use`).
  - Under `--unused-deps`, the placement rules (`type-only-dependencies`, `test-only-dependencies`, `dev-dependencies-in-production`) give no note. The two rules that production mode turns off give no note either.

  The exit code does not change, so a script that passes `--unused-deps` with the rule off still exits 0. To fail such a script on a finding, set the rule to "error". A rule at "warn" reports the finding and does not fail the run. `--quiet` removes the stderr note, and the machine formats still carry the entry. Thanks @Smrtnyk for the report. (Closes [#4067](https://github.com/fallow-rs/fallow/issues/4067))
