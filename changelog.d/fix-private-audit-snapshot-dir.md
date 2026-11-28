### Changed

- **`fallow audit` keeps its base snapshots in a directory of the user on Unix.** The base snapshot of `--base` and the staged tree of `--staged` were directories directly in the temp dir. They are now in `fallow-audit-snapshots-<uid>` inside the temp dir, with mode 0700. Fallow does not use that path when it is a link, is not a directory, belongs to another user, or gives access to other users: the audit then prints a warning and stops with exit code 2. The cache sweep and `fallow audit-cache prune` and `remove` also visit the snapshots that earlier versions left in the temp dir. Windows keeps the snapshots in the temp dir, which is already private to the user.
- **A walk from the top level of a repository applies no `.ignore` file from a directory above it.** Git already applies no `.gitignore` from above a repository. Before, a `.ignore` file above the repository top level, for example in the temp dir above an audit base snapshot, could hide files from the analysis. A root below the top level still applies the ignore files of the repository.

### Fixed

- **The audit cache reads its `.sha` and `.last-used` files only when the user owns them.** A file of another user at one of these paths now reads as absent, so the audit rebuilds the snapshot.
