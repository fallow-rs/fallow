### Fixed

- **The composite action runs its scripts on Windows runners.** Every `run:` step now quotes `GITHUB_ACTION_PATH`, so bash no longer removes the backslashes from the Windows action path before it resolves `action/scripts/*.sh`. (Closes [#5352](https://github.com/fallow-rs/fallow/issues/5352))
