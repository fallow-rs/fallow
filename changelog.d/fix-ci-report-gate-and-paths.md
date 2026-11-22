### Fixed

- **A sticky PR or MR comment is created when only an enforced gate fails.**
  Before, the comment envelope marked such a run as clean when the diff filter
  dropped every finding. The post step then skipped the first comment, while
  the heading of the comment said that the gate failed. `is_clean` in the
  envelope now follows the gate verdict of the heading.
- **`fallow audit --format codeclimate` applies the report path prefix.**
  Before, the live audit output kept analysis-root-relative paths when `--root`
  was below the repository root or `--report-path-prefix` was set. The paths
  now match the CodeClimate output of the other commands and of
  `fallow report`.
