### Added

- **Built-in gulp plugin.** When a package depends on `gulp` or `gulp-cli`, fallow makes its `gulpfile` an entry point. This includes `gulpfile.{js,mjs,cjs,ts,mts,cts,babel.js,esm.js}` and the `index` file of a `gulpfile.js` directory. The task files that a gulpfile imports are no longer unused files.
