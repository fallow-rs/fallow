### Fixed

- **Warnings on a pipe or in a log file no longer carry color codes.** The
  tracing warnings on stderr were colored for every writer. An agent or a CI
  log then got escape codes such as `\x1b[33m` around each `WARN` line. Color
  now stays on only for a terminal, and `NO_COLOR` turns it off.
  `CLICOLOR_FORCE` turns it on, the same as for the report.
- **The setup hint of a bare run names the current setup commands.** The hint
  now names `fallow init` for a config and `fallow agent install --dry-run`
  for the agent guide, skills, MCP server, and commit gate. The JSON `setup`
  next step now has the same offer: its command is
  `fallow agent install --dry-run`, not `fallow schema`.
- **`fallow init` names the next step.** After it writes the config, `init`
  tells you to run `fallow` for a first report. When the config root is not
  the current directory, the command includes `--root`. `--quiet` drops this
  line.
- **`fallow agent install --dry-run` names the command that applies the plan.**
  A dry run that plans writes puts `agent-install-apply` first in
  `next_actions[]`, also in the human `Next:` list. The command repeats the
  flags that change the plan, without `--dry-run`. A harness that detection
  found is repeated as `--harness`, so a run in another shell writes the
  same plan.
- **The review focus no longer calls one importer "high fan-in".** The reason
  now reads `fan-in (1 importer)`, the same form for every count.

### Changed

- **The `fallow` skill shows the review loop that fallow checks.** One short
  example runs `fallow audit`, reads the walkthrough guide, writes a judgment,
  and validates it with `--walkthrough-file`. It saves the guide and the
  judgment in the temporary folder, because a new file in the project tree
  makes the judgment stale.
