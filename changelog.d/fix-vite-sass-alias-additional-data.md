### Fixed

- **Vite projects no longer report Sass files that Vite loads without an import as unused.**
  Three cases caused false findings. An `@use` in
  `css.preprocessorOptions.scss.additionalData` now credits the file that Sass
  loads when the specifier has no extension, leaves out the `_` of a partial, or
  names a directory with an `_index` or `index` file (#3819). An `@use` in a Sass
  file now resolves through a Vite `resolve.alias` entry, with the Sass partial
  and directory-index rules. Before, Fallow reported the import as unresolved
  and the target partial as unused (#3820). The Sass settings file that
  `vite-plugin-vuetify` reads from `styles.configFile` is now an entry. Fallow
  reads the path relative to the Vite `root` (#3821). Thanks
  [@Smrtnyk](https://github.com/Smrtnyk) for the three reports.
