### Changed

- **Pull request jobs miss the Rust cache less often.** The repository uses more than the GitHub cache limit, so GitHub evicts the oldest caches within hours. Daily and weekly workflows saved large Rust caches that were gone before their next run and evicted the caches that pull request jobs restore. Workflows without a push trigger now restore a cache but do not save one. Releases keep their caches. This change affects contributors only.
