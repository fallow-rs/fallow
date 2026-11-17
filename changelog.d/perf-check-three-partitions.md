### Changed

- **Rust pull requests get their CI result sooner.** The nextest suite now runs in three `Check` jobs instead of two. Each job still builds every test binary, but it runs a third of the tests, so the slowest `Check` job, the last job of most pull request runs, finishes earlier. The same tests run. This change affects contributors only.
