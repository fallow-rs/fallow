### Fixed

- **`fallow health --css` no longer reports classes that code, a dependency or a theme adds at runtime.** Before, `unreferenced-css-class` reported a class when project markup did not write it, also when other code applied it. Fallow now credits these classes:
  - a class name in a string literal of a statement that names a class, for example `const ROW_MARKER_CLASS = "table-row-drag-marker"` or `classes.push("cell-border-top")`. The static head of a template literal in such a statement credits a prefix, for example `` `cell-edge-${side}` ``.
  - a class that a declared dependency renders: ProseMirror and its tables, the Tiptap placeholder and node views, and BProgress.
  - a class of the Docusaurus classic theme and its Infima styles, when the package declares `@docusaurus/preset-classic` or `@docusaurus/theme-classic`.
  - a class that starts with the name of a declared package, for example `maplibregl-popup` for `maplibre-gl`.

  Fallow now reads the dependencies of each workspace `package.json`, not only the root `package.json`. A dependency credits a class only in the stylesheets of the workspace that declares it.
  (Closes [#5272](https://github.com/fallow-rs/fallow/issues/5272))
