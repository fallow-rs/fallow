### Changed

- **Storybook, Vitest, Rstest, Jasmine and Playwright globs now accept every
  extglob group.** One shared rewrite now reads the groups `@(..)`, `?(..)`,
  `+(..)`, `*(..)` and `!(..)` for these plugins and for `import.meta.glob`.
  Before, each plugin had its own rewrite with its own gaps. The changes per
  plugin are:
  - Storybook: before, only `@(..)` matched. A `stories` glob such as
    `../src/**/!(*.dev).stories.@(js|ts)` matched no file. Now every group
    matches, and `!(*.dev)` excludes the files that match `*.dev`.
  - Playwright: before, a `testMatch` glob with a group made every script
    below `testDir` an entry point. Now only the files that the glob matches
    are entry points. The match still ignores case.
  - Vitest, Rstest and Jasmine: `!(..)` and `*(..)` now match. A Vitest plain
    `(a|b)` group still matches one of its alternatives.
  - Vitest: before, a `test.include` glob with any group made no file an
    entry point, so Fallow reported every test file as unused. Now the
    `test.include` globs of the config and of each project use the shared
    rewrite.
- **A `+(..)` group in an include glob now matches one or more repetitions.**
  The glob matcher cannot repeat a group, so `+(ts|js)` becomes `{ts,js}*`.
  This can make a few more files entry points, for example `a.test.jsx` for
  `*.test.+(ts|js)`. A `+(..)` group at the end of a glob also matches a
  longer extension, so `*.test.+(ts|js)` also matches `a.test.json` and
  `a.test.js.map`. Before, Vitest, Rstest and Jasmine matched the group
  once only, which missed names that repeat the group. An exclude glob still
  matches a `+(..)` group once only, so an exclude never removes a file that
  the tool keeps.
