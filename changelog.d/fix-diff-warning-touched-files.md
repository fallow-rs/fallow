### Fixed

- **The `--diff-file` warning for a diff with no added lines no longer says
  that no finding passes.** A pure rename still touches the renamed file, so
  an unused file or a cycle in that file stays in the report, and dependency
  findings never go through the line filter. The warning now names both.
