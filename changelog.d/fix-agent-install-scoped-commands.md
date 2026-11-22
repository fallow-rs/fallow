### Fixed

- **The follow-up commands of `fallow agent install` and `fallow agent status` keep the inspected project.** Before, the `claude-approve-mcp`, `choose-harness`, `recommend-config`, `agent-install`, `agent-install-force` and `gate-script-refresh` commands did not repeat `--root`. The approve command also did not repeat `--without`. From another directory, these commands acted on the wrong project, and the approve command wrote the excluded steps. Now each command repeats the `--root` of the run, and the install commands also repeat the excluded steps.
