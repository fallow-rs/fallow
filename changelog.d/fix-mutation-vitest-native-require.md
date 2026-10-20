### Fixed

- **`fallow mutation` now instruments a file that a Vitest test loads with
  a native `require`.** A test that loads a CommonJS file through
  `createRequire(import.meta.url)` ran the original file. The report then
  listed the file in `unloaded_paths` and gave each of its mutants
  `no_coverage`. The Vitest setup file now serves the instrumented file to
  such a load on Node 22.15 and later. On an older Node, the file stays in
  `unloaded_paths`, and a static `import` is the workaround.
  (Closes [#3653](https://github.com/fallow-rs/fallow/issues/3653))
