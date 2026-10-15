### Fixed

- **A `.then()` callback on a dynamic `import()` credits the exports that it
  reads.** Before, Fallow ignored the callback param in shapes such as
  `import('./x').then((m) => m.start())`, `.then((m) => { m.start(); })` and
  `.then(function (m) { m.start(); })`. The exports read on `m` then reported
  as unused, and so did the types that only those exports use. Now each
  member that the callback reads on the param is credited. When the callback
  hands the param to other code, for example `.then((m) => register(m))`,
  every export of the target module is credited. The shapes
  `.then((m) => m.start)` and `.then(({ start }) => start())` already worked.
  The extraction and graph cache versions change, so the first run after the
  upgrade parses the project again.
