### Fixed

- **`fallow dead-code` credits the packages in tsconfig `contentMappers`.** TypeScript 7.1 loads each `contentMappers[].package` to map files such as `.vue` or `.astro` into the program. Fallow did not read this field, so it reported the mapper package as an unused dependency. The TypeScript plugin now credits the package name. Relative and absolute paths credit no dependency. Thanks to @Smrtnyk for the report.
  (Closes [#3870](https://github.com/fallow-rs/fallow/issues/3870))
