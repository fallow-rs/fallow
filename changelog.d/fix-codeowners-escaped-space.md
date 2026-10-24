### Fixed

- **A CODEOWNERS pattern with an escaped space now parses.**
  GitLab writes a space in a path as `\ `, for example
  `/docs/Security\ Release.md @owner`. Before, Fallow split the line at that
  space, and `--group-by owner` and `--group-by section` failed with
  "dangling '\'". GitLab's own CODEOWNERS file failed in this way. A
  backslash escape such as `\#` now also works on Windows.
