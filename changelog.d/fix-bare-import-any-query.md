### Fixed

- **A bare import with any query credits a package that is not installed.** Bundler plugins define their own queries, for example the esbuild `?binary` loader in `import('pkg/mod.wasm?binary')`. Before, only the Vite queries such as `?url` and `?raw` credited the package. Other queries gave an unresolved import and an unused dependency. A missing file of an installed package stays an unresolved import with the full specifier.
  (Closes [#4732](https://github.com/fallow-rs/fallow/issues/4732))
