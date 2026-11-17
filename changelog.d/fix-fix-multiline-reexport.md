### Fixed

- **`fallow fix` now removes unused names from an export list that spans several lines.** A formatter puts each name of a long `export { ... } from` list on its own line. `fix` removed the `export` of the source declaration but kept the list, so the type check of the fixed code failed with TS2459. `fix` now removes the unused names from the list and keeps the layout, comments and trailing commas of the names that stay. It also removes a comment line above a removed name when no kept name follows that comment. It deletes the full statement when no name is left. It also removes an aliased name such as `export { a as b }` by its exported name.
  (Closes [#4636](https://github.com/fallow-rs/fallow/issues/4636))

### Added

- **`fallow fix` reports each export that a kept re-export holds back.** `fix` no longer removes the `export` of a declaration while a re-export that it keeps still names that declaration. A re-export stays, for example, in a file that `fix` skips as low confidence, in a file whose names `ignoreExports` hides, or in a list statement that has no safe text edit. Each withheld removal is a `remove_export` entry with `skip_reason: "kept_re_export"`. The new JSON counter `skipped_kept_re_exports` counts these entries, and the human output and the CI step summary state them. The exit code does not change. Limit: a file that `ignorePatterns` excludes is not in the module graph, so a re-export in such a file cannot hold a removal back. Run the type check after `fallow fix` when an ignored file re-exports project code.
  (Refs [#4636](https://github.com/fallow-rs/fallow/issues/4636))
