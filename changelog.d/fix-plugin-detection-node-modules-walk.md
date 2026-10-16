### Performance

- **Plugin detection no longer walks `node_modules` for a Vitest, Jest or
  Rstest project glob.** Since 3.33.1, the Vitest plugin expands each glob
  entry in `test.projects` on disk. A glob with `**` after its base, for
  example `apps/**/vitest.config.ts`, followed each symlink in the
  `node_modules` of each workspace. In a pnpm monorepo, the walk then read the
  full dependency store once for each workspace, and plugin detection took
  minutes. The walk now does not enter `node_modules`, `.git`, or a
  gitignored path, such as a local build output. It still follows a symlinked
  project directory, and it stops at a symlink loop. A limit on the number of
  read directories bounds each glob, and fallow shows a warning when a glob
  reaches it. The Jest `projects` and Rstest `projects` globs use the same
  walk.
  Thanks [@wouterkroes](https://github.com/wouterkroes) for the report.
  (Closes [#3532](https://github.com/fallow-rs/fallow/issues/3532))
