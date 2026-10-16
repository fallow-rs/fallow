### Fixed

- **`export default x` and `export = x` of an imported binding now forward the
  import.** Before, Fallow recorded these statements as a new local export.
  The importers of the forwarding file were then not credited to the original
  export, so `--trace` and `--impact-closure` did not list them, and search
  visibility kept the forwarding file as the only consumer. Now both forms
  make the same re-export of `default` as `export { x as default }`. The
  forwarding file is `pass-through`, and the importers show in the trace. For
  `export = x`, a member read on an `import m = require(..)` binding of the
  file, such as `m.run()`, also credits the forwarded default export. The
  extraction and graph cache versions change, so the first run after the
  upgrade parses the project again.
  (Closes [#3392](https://github.com/fallow-rs/fallow/issues/3392))
- **A re-export inside a webpack asset loader resource no longer keeps its
  source in use.** A `raw-loader!` resource never runs. Its imports already
  stayed out of reach, but a re-export in it, such as
  `export { local as default }`, still credited the source file, so that
  file did not show as unused. Now a module that only asset loader edges load
  forwards nothing through its re-exports.
