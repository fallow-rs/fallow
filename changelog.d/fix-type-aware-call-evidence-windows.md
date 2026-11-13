### Fixed

- **`constant-parameter` now reads type evidence on Windows.** With `--type-aware`, the rule gave no `unused-default` finding and no method finding on Windows, and each candidate had the abstention reason `no-project`. The type-aware companion compared a request path that has backslashes with a checker file name that has forward slashes. The companion now compares the resolved identity of both files.
