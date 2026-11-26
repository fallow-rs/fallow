### Performance

- **A warm run after an edit that keeps the import statements no longer
  resolves the imports of the project again.** The module graph cache now
  holds a second hash for each file, over the facts that import resolution
  reads. When the content of a file changed, and no import, re-export,
  `require()` call, `import()` call or module mock of any file changed or
  moved, the run takes the resolved imports from the cache and builds the
  graph from the edited modules. An edit above a `require()` or `import()`
  call moves that call, so the run resolves again. In the `--performance`
  table a run that reuses the resolved imports shows
  `resolve imports: 0.0ms` together with the line `graph cache not reused:
  at least one file changed`. A project with convention auto-imports resolves again after
  each edit, as before. The first run after the upgrade discards the old
  graph cache once.

### Fixed

- **A graph cache that a run writes after it reuses the cached import
  resolution now keeps the versions of the hoisted Bun packages.** Such a
  run wrote the cache again without the installed versions that the
  resolution had read. A later `bun install` that replaced a hoisted package
  under the same name was then not seen.
