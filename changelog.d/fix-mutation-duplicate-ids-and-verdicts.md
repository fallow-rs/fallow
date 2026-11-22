### Fixed

- **`fallow mutation --verdicts` updates the function of the judged mutant.** Two callbacks on one line give two `functions[]` entries with the same file, name and first line. An equivalent verdict for a mutant of the second callback changed the counts of the first one. The join now finds a survivor by the `survived` list of its function. For another status it prefers the function whose line range holds the mutant. Two callbacks that start and end on the same line still share one key, because the report has no column range for a function.

- **`fallow mutation --kill-matrix` no longer calls a test with a duplicate id redundant.** The runner numbers tests that share a title in report order, so the kills of such a test can belong to its twin. The full kill matrix still lists the row, but `kill_matrix.candidates`, `coverage_without_kills` and `subsuming_tests` leave out the duplicate ids that the dry run reports. `--kill-matrix-for` already left them out.
