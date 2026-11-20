### Fixed

- **An expired or invalid license no longer blocks a single local runtime coverage capture.** A single local capture needs no license, but `--runtime-coverage` stopped with exit code 3 when the license file was expired or invalid. Fallow now prints the license problem as a warning and runs the analysis without the license. A run with more than one capture still needs a valid license and still exits 3.
  (Closes [#4843](https://github.com/fallow-rs/fallow/issues/4843))
- **A function without V8 tracking no longer gets the `safe_to_delete` verdict.** The verdict needs V8 tracking. When `evidence.v8_tracking` is `untracked`, the local runtime coverage report now gives `review_required` with the `review-runtime` action.
  (Closes [#4843](https://github.com/fallow-rs/fallow/issues/4843))
