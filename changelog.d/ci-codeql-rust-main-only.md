### Changed

- **Pull request checks finish sooner.** The Rust CodeQL analysis takes about 20 minutes and ran longer than CI on most pull requests. It now runs on pushes to `main` and on the weekly schedule. A pull request analyzes Rust only when it changes the CodeQL configuration. The JavaScript and TypeScript analysis and the Actions analysis still run on each pull request. On `main`, a newer push replaces a queued analysis, and a started analysis always finishes. This change affects contributors only.
