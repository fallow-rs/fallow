### Fixed

- **Audit cleanup preserves unrelated Git worktree registrations.** Removing legacy
  Fallow snapshots keeps user worktrees registered when their directories are
  temporarily unavailable.
  (Closes [#3884](https://github.com/fallow-rs/fallow/issues/3884))
