### Fixed

- **A focused duplication run reports its totals against the full project.** On a project with 1024 or more files, a run that analyzes only changed files (for example `fallow dupes --changed-since`) reported `total_files`, `total_lines`, `total_tokens` and `duplication_percentage` against a part of the files. The run now reports them against all files, as a full run does. The percentage of such a run can thus be lower than before.
