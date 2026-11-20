### Added

- **`fallow schema` tells source edits apart from report writes.** Each `mcp_tools` row and each `fallow://tools` row has a new `modifies_source` field. It is `true` only for `fix_apply`, which edits source files. `analyze`, `check_changed`, `find_dupes` and `check_health` keep `read_only: false`, because they write a report or baseline file on request, and they have `modifies_source: false`. The `read_only` field and the MCP `readOnlyHint` annotation do not change.
  (Refs [#4845](https://github.com/fallow-rs/fallow/issues/4845))
