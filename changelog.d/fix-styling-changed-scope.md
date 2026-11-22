### Fixed

- **`fallow audit` keeps the styling findings of changed files.**
  The deep CSS pass of `fallow audit` (on by default, `audit.cssDeep`) scans the whole project but reports only the changed files. A Tailwind arbitrary value that an unchanged file used first was dropped, also when a changed file used it too. The report now points at the first use in a changed file. Raw style values of unchanged files also filled the limit of 200 reported values, so the values of changed files did not show. Now only the values of changed files count toward the limit. The values of unchanged files still count as the project vocabulary for the nearest-token match.
- **An `@plugin` inside a `/* */` comment or a string no longer stops the theme token checks.**
  fallow found `@plugin` anywhere in a stylesheet, also in a comment or a string. The checks for unused, near-duplicate and consumed theme tokens then stopped for the whole project. Now only a real `@plugin` at-rule stops them.
