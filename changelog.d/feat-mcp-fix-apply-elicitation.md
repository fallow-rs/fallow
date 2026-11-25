### Added

- **`fix_apply` asks the user to confirm before it writes.** When the MCP client declares form elicitation, `fix_apply` first shows the dry-run plan in one form: the number of fixes, the number of files, and the fixes per type. The tool writes only after the user confirms. On a decline, a cancel or a timeout, no file changes, and the result has `applied: false` with a `confirmation` value that gives the reason. On protocol `2026-07-28` the form arrives through `input_required`, and the client calls the tool again with the answer. A client without form elicitation keeps the old behavior. A headless session, such as `claude -p`, cancels the form at once, so the call does not hang.
  (Closes [#5238](https://github.com/fallow-rs/fallow/issues/5238))
