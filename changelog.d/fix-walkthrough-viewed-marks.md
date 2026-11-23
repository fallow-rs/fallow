### Fixed

- **Walkthroughs record `--mark-viewed` in JSON output as well as in human and Markdown output.** Absolute paths and paths with `.` or `..` mark the same file as the guide path. A mark that matches no staged file prints a warning on stderr.
