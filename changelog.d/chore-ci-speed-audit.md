### Changed

- CI skips Rust Structure analysis for pull requests whose complete diff changes only known prose or frontend source. Rust inputs and uncertain diffs retain the full checks, including file-growth enforcement. Main and release pushes always run the analysis.
