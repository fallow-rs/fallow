### Fixed

- **A small `.js` file with JSX and a CommonJS file with a top-level `return`
  now parse without errors.** The JSX retry parse now also runs for a file of
  100 bytes or less when the first parse reports an error. A top-level
  `return` is now valid in a `.js`, `.jsx`, `.ts` or `.tsx` file, because
  Node.js wraps a CommonJS module in a function. Fallow no longer marks these
  files as `source-parse-degraded`, and their imports, exports and calls now
  count.
  (Closes [#4497](https://github.com/fallow-rs/fallow/issues/4497))
