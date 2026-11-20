### Fixed

- **`fallow fix` no longer deletes an unrelated setting of
  `pnpm-workspace.yaml`.** When one run removed an unused catalog entry and an
  empty catalog group, the group removal used a line number from before the
  entry removal. It then deleted the next top-level setting, for example
  `linkWorkspacePackages: true`.
- **An emptied catalog with an inline comment on its header stays a
  mapping.** `fallow fix` and the editor quick fix wrote `ui: # note {}`, so
  the catalog was null and `pnpm install` failed. They now write
  `ui: {} # note`.
- **The editor quick fix checks the unsaved buffer before it removes a catalog
  group or entry.** It no longer offers to delete a catalog group that has
  entries in the buffer, or an entry that belongs to another catalog.
- **A catalog entry with a blank line after its key is removed in full.**
  `fallow fix` and the editor quick fix left the value lines of the entry in
  the file.

### Changed

- **`fallow fix --format json` has new skip reasons for catalog records.** It
  no longer emits `skip_reason: "line_out_of_range"`. It emits `not_found`,
  `not_empty`, `unsupported_yaml_style` and `unsafe_edit`. An edit that would
  change other parts of `pnpm-workspace.yaml` is now a skip with
  `unsafe_edit`, not a write error with exit code 2.
