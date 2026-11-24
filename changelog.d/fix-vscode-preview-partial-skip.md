### Fixed

- **The VS Code fix preview shows the available fixes when the dry run skips a file.** Before, `Fallow: Fix` showed only an error when `fallow fix --dry-run` exited with code 2 because a file had mixed line endings or changed during the run. The preview now offers the eligible fixes, and a warning explains what the run skipped. (Closes [#5126](https://github.com/fallow-rs/fallow/issues/5126))
