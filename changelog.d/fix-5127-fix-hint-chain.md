### Fixed

- **`fallow fix` now names the diagnostic that holds back a fix, with the step that unblocks it.** When a fix rests on a file or folder that the run did not fully analyze, the help said only "Resolve the workspace diagnostics". That list also held diagnostics that hold back no fix, such as `undeclared-workspace`. Now each held-back entry names the path and the config change, for example "add `!.claude/**` to `ignorePatterns`". The JSON envelope has a new `incomplete_analysis_diagnostics` array with each blocking diagnostic and its `action`. The human output lists them also with `--quiet`.
- **`fallow workspaces` now shows the diagnostics of the source file walk.** It showed only the workspace diagnostics, so a skipped hidden folder with sources (`skipped-source-dotdir`) was not in the list. The command now runs the file walk and reports the same `workspace_diagnostics` as `fallow list`.
- **The `undeclared-workspace` hint also names `workspaces.patterns` in the fallow config**, for a folder that does not belong in `package.json` or `pnpm-workspace.yaml`.
- **VS Code: an auto-fix run that applies no fix now shows why.** The warning shows the reason that held back the most fixes, with its next step.
  (Closes [#5127](https://github.com/fallow-rs/fallow/issues/5127))
