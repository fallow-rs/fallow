### Fixed

- **`--sarif-file` now exits 2 on a command that writes no SARIF file.** Only bare `fallow`, `fallow dead-code`, `fallow architecture` and `fallow security` write the file. Before, `fallow audit --sarif-file out.sarif` and other commands accepted the flag, exited 0 and wrote nothing. Now the command prints one error line and exits 2. To get SARIF from `fallow audit`, use `--format sarif` with `--output-file`.
  (Refs [#4845](https://github.com/fallow-rs/fallow/issues/4845))
- **The GitHub Action and the GitLab template now add the workspace name to the sticky comment marker.** With `workspace: app` and no `comment-id` (or `FALLOW_COMMENT_ID`), the marker is now `fallow-results-app`, as the input descriptions state. Before, the scripts always passed `fallow-results`, so parallel jobs for different workspaces edited the same comment. Upgrade note: a job that sets a workspace and no comment id gets a new marker. The first run after the upgrade posts a new comment and leaves the old comment in place. Delete the old comment, or set `comment-id: fallow-results` to keep it.
  (Refs [#4845](https://github.com/fallow-rs/fallow/issues/4845))
