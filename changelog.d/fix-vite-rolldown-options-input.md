### Fixed

- **`fallow dead-code` now reads `build.rolldownOptions.input` in a Vite config.** Vite 8 names the bundler options `rolldownOptions` and keeps `rollupOptions` as an alias. Fallow read only the old name, so each file that only such an entry reaches was a false `unused-file`. The Vite plugin now reads `input` under both names, in `build` and in `environments.<name>.build`, as a string, an array or an object. The Electron plugin does the same for each section of an electron-vite config.
  (Closes [#5385](https://github.com/fallow-rs/fallow/issues/5385))
