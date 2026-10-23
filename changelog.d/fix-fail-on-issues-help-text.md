### Fixed

- **The `--fail-on-issues` help text now says where the flag changes the exit code.**
  Bare `fallow` in a json, sarif or codeclimate format and `fallow dupes` exit
  0 on issues unless `--fail-on-issues` or `--ci` is set. The human, compact
  and markdown formats of a bare run, and the standalone analysis commands,
  already exit 1 on error-severity issues. The exit codes do not change.
  Thanks [@Smrtnyk](https://github.com/Smrtnyk) for the report in #3826.
