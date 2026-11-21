### Fixed

- **`fallow fix` no longer puts an enum member or export edit on the wrong line.** When an earlier edit of the same run removed lines above it, the fixer looked up the line of the finding in the changed text. The edit then landed on the wrong line or was lost. Each source fixer now plans its edits against the text that the analysis read, and `fallow fix` applies all edits of a file one time.

### Changed

- **`fallow fix --dry-run` reports an unused exported enum with only unused members one time.** Before, the preview listed two `remove_export` records for such an enum: one for the unused export and one for its members. The apply run removes the enum one time, so the preview now shows one record.
