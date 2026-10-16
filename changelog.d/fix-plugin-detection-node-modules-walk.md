### Performance

- **Plugin detection no longer walks `node_modules` for a Vitest, Jest or
  Rstest project glob.** Since 3.33.1, the Vitest plugin expands each glob
  entry in `test.projects` on disk. A glob with `**` after its base, for
  example `apps/**/vitest.config.ts`, followed each symlink in the
  `node_modules` of each workspace. In a pnpm monorepo, the walk then read the
  full dependency store once for each workspace, and plugin detection took
  minutes. The walk now does not follow symlinks and does not enter
  `node_modules` or `.git`. A limit on the read directory entries bounds each
  glob. The Jest `projects` and Rstest `projects` globs use the same walk.
  Thanks [@wouterkroes](https://github.com/wouterkroes) for the report.
  (Closes [#3532](https://github.com/fallow-rs/fallow/issues/3532))
