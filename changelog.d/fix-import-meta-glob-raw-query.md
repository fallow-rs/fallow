### Fixed

- **`fallow dead-code` now reports unused exports in files that an `import.meta.glob` reads as text or a URL.** A glob with a `?raw`, `?url` or `?inline` query, or with the legacy `as: 'raw'` or `as: 'url'` option, keeps its matches in use but credits none of their exports. A glob with the `import` option, such as `import: 'default'`, credits only that export. A glob with a query that fallow does not know still credits every export.
  (Closes [#4532](https://github.com/fallow-rs/fallow/issues/4532))
