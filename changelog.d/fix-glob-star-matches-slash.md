### Fixed

- **A single `*` in `entry` and `dynamicallyLoaded` globs no longer matches `/`.**
  The pattern `src/plugins/*/index.js` also matched nested files such as
  `src/plugins/alpha/components/button/index.js`. Each nested match became an
  entry point, so fallow did not report it as an unused file. A single `*` now
  stays within one path segment and only `**` crosses directories, as in
  gitignore, picomatch, and the plugin entry patterns. If your config used a
  single `*` to match nested files, change it to `**`. Other glob fields,
  such as `ignorePatterns`, keep their current matching. Thanks
  [@jleifeld](https://github.com/jleifeld) for the report.
  (Closes [#3490](https://github.com/fallow-rs/fallow/issues/3490))
