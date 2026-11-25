### Fixed

- **`--trace File.svelte:default` no longer says UNUSED for a component that a file imports.** A `.vue`, `.svelte` or `.astro` file without an explicit `export default` has no export entry, so the trace found no references, while dead-code did not report the component. The trace now lists the files that import the component by its default or by a namespace import. A reachable component file is used, because dead-code reports an unused component as an unused file.
  (Closes [#5172](https://github.com/fallow-rs/fallow/issues/5172))
