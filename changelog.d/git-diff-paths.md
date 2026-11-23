### Fixed

- **`--changed-since` finds changed and renamed files when Git uses relative paths.** Runs from a repository subdirectory now use the correct file paths when `diff.relative` is set to `true`.
