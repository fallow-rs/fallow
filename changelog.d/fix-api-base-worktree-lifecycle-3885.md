### Fixed

- **Programmatic and MCP audits no longer show their base snapshot in `git worktree list`.**
  The API, the MCP `audit` tool, and the decision surface now remove the Git
  registration of the base snapshot before the base analysis starts, as
  `fallow audit` does. A host process that stops during the analysis leaves no
  registered worktree behind.
  (Closes [#3885](https://github.com/fallow-rs/fallow/issues/3885))
