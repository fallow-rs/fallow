### Fixed

- **`fallow mutation` prunes the config object of each plugin config file.**
  Some plugins list their config file as always used, not as a config glob,
  for example `.vitepress/config.ts`, `remix.config.js` and `nitro.config.ts`.
  The mutants in the default export of such a file were `no_coverage`, because
  no test loads a config file, and they lowered the coverage. They now get the
  prune reason `framework-config`, the same as `vite.config.ts`.
