### Changed

- **One table decides each global option per command.** A new table states for each global option whether each command uses it, ignores it or rejects it. The startup check reads this table and exits with code 2 for a rejected pair. The baseline, regression, output, gate, `--lean`, `--group`, `--fail-on-parse-error` and `--sarif-file` checks, and the option lists of `security`, `doctor` and `similar-code`, now read it. The error messages are the same. `fallow schema --cli-reference` exports the table as the additive `command_support` field of each global option. These command lines accepted the option with no effect and now exit with code 2:
  - `fallow schema`, `config-schema`, `plugin-schema` and `rule-pack-schema` with `--ci`, `--fail-on-issues`, `--sarif-file` or `--output-file`.
  - `fallow schema`, `config-schema`, `plugin-schema`, `rule-pack-schema` and `telemetry` with `--group`.
  - `fallow telemetry` and `fallow impact statusline` with `--sarif-file`, `--fail-on-regression`, `--regression-baseline`, `--save-regression-baseline` or `--tolerance`.
  - `fallow schema`, `config-schema`, `plugin-schema` and `rule-pack-schema` with `--fail-on-regression`, `--regression-baseline`, `--save-regression-baseline` or `--tolerance`.
  - `fallow telemetry --output-file`.
  - `fallow agent`, `fix`, `impact`, `recommend`, `rule-pack`, `suppressions`, `viz` and `workspaces` with `--output-file`. These commands print their output to stdout and left an empty file. In JSON format, `agent status` and `rule-pack list` wrote the file; they now also exit with code 2.
  - `fallow impact statusline` with `--ci` or `--fail-on-issues`.
  - `fallow workspaces` with `--fail-on-regression`, `--regression-baseline`, `--save-regression-baseline` or `--tolerance`.
  - `--tolerance 0` on `fallow doctor` and on each command without a regression baseline. The earlier checks compared the value with `0` and so missed an explicit `0`.

  A test now fails when a command ignores an option that names a file, such as `--save-regression-baseline` on `fallow dupes` (issue #4931).

  The check runs before the root, git ref and diff validation, and it reads the options in the order of their definition, with `--tolerance` after the other regression options. When a command line has more than one problem, the first error can be a different one. A rejected option now records a `Validation` failure in the telemetry of the run. The help of `fallow security` now also hides `--tolerance`, which `security` ignores.
