### Fixed

- **`--workspace '!<nested>'` now excludes a workspace below a selected workspace.** `dead-code`, `dupes`, `health`, `security`, `flags`, `suppressions` and `audit` reported the files of the excluded workspace, because it is below a selected root.
- **A nested workspace that no `workspaces.settings` key selects uses the global settings.** It used the `ignoreDependencies` and `includeEntryExports` values of its parent workspace.
