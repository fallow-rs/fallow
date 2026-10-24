### Fixed

- **A CODEOWNERS pattern with an escaped space now parses.**
  GitLab writes a space in a path as `\ `, for example
  `/docs/Security\ Release.md @owner`. Before, Fallow split the line at that
  space, and `--group-by owner` and `--group-by section` failed with
  "dangling '\'". GitLab's own CODEOWNERS file failed in this way. On Windows, a
  backslash in a pattern is now an escape, as on other platforms, and no
  longer a path separator. Thus `\ ` and `\#` now work on Windows too.
