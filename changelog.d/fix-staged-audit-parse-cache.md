### Fixed

- **`fallow audit --staged` reuses the parse cache of the checkout.** The
  staged audit analyzes a new temporary worktree on each run, so it read an
  empty cache and parsed every file again. The run now starts from the parse
  cache of the checkout and writes the updated cache back, so a second staged
  run parses only the files whose staged content changed. The cache keys each
  entry by its content hash, so a staged file that differs from the working
  tree is still parsed from the index.
  (Closes [#4763](https://github.com/fallow-rs/fallow/issues/4763))

- **`fallow audit` with a shared `FALLOW_CACHE_DIR` keeps the caches of the
  checkout valid.** The base pass and the staged pass analyze temporary
  worktrees. They wrote their module graph cache into the shared directory,
  so the next run of the checkout rejected the graph cache with
  `root-mismatch` and built the graph again. These passes now keep their
  caches in their own worktree.
