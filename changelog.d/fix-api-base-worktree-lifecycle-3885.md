### Fixed

- **Programmatic and MCP audits no longer show their base snapshot in `git worktree list`.**
  The API, the MCP `audit` tool, and the decision surface now remove the Git
  registration of the base snapshot before the base analysis starts, as
  `fallow audit` does. They also remove the snapshots that a stopped process
  left behind. A snapshot whose owner process can still run stays.
  (Closes [#3885](https://github.com/fallow-rs/fallow/issues/3885))

### Security

- **Git commands in an audit base snapshot no longer read a repository from the base commit.**
  The `.git` file of a base snapshot pointed at a relative path. A base commit
  could supply a directory at that path, or a directory shaped as a bare
  repository, and Git then read its config. That config can run commands, for
  example through `core.fsmonitor`. The `.git` file now names an absolute path
  that cannot exist, and each Git command that Fallow runs inside a snapshot
  gets a `GIT_DIR` that does not exist.
