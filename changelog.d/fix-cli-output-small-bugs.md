### Fixed

- **`fallow rule-pack init`, `list` and `test` show the noun again.** The human output printed `3 s` for three rules and `1 ` for one finding. It now prints `3 rules` and `1 finding`.
- **`fallow hooks install --target agent --gitignore-claude` reports the real `.gitignore` change.** When `.gitignore` already lists `.claude/`, the summary line says `unchanged (.claude/ already listed)`. Before, it said `.claude/ added` and the file did not change. A dry run now also shows the `.gitignore` line, with `would add .claude/` or `unchanged`.
- **A dry run of `fallow hooks install --target agent` says `would create` for the hook config file.** The `.claude/settings.json` and `.codex/hooks.json` lines said `created` (or `updated`) although the dry run writes no file. They now use the same `would ...` wording as the other dry-run lines.
- **`fallow ci-template gitlab | head` exits quietly.** A closed stdout reader made the command abort with a panic. The command now stops the output and exits with code 0, as the other commands do.
- **The help of `fallow ci-template gitlab --vendor` names only the files that it writes.** The help said that the flag also writes `action/` files. The flag writes only the `ci/` files.
- **The output schema describes `is_internal_dependency` of `fallow workspaces` correctly.** The field is `true` when another workspace of the project lists the package as a dependency.
