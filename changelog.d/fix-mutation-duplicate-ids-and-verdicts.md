### Fixed

- **`fallow mutation --verdicts` updates the function of the judged mutant.** Two callbacks on one line give two `functions[]` entries with the same file, name and first line. An equivalent verdict for a mutant of the second callback changed the counts of the first one. The join now finds a survivor by the `survived` list of its function, and another mutant by the line range of its function.
