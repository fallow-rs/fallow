### Fixed

- **The mutation result cache now keys on the `bunfig.toml` preload files when no package enables the Bun plugin.** Before, only a project with `@types/bun` or `bun-types` gave its preload files to the key. In another project, a change to a preload file kept the stored results, so a `bun test` run could report a stale `killed` or `survived` status. Each `preload` and `[test] preload` file of the root `bunfig.toml`, and each file that it imports, is now a key part.
