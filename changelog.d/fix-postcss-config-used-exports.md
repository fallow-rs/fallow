### Fixed

- **The `default` export of `postcss.config.*` is no longer reported as an
  unused export under `includeEntryExports`.** PostCSS reads that export, as
  Vite reads the default export of `vite.config.*`, which fallow already
  credited.
