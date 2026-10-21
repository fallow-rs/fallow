### Fixed

- **More tool config files keep their exports under `includeEntryExports`.**
  Fallow no longer reports the `default` export of these config files as
  unused: Capacitor `capacitor.config.*`, Sanity `sanity.config.*` and
  `sanity.cli.*`, Nitro, SVGO, SVGR (`svgr.config.*` and `.svgrrc.js`),
  Kysely, OpenAPI TS, Remix, i18next `i18next.config.*`, VitePress
  `.vitepress/config.*`, CSpell `cspell.config.*`, `@sanity/pkg-utils`
  `package.config.*`, Electron Forge `forge.config.*` and electron-builder
  `electron-builder.{js,cjs,ts}`. A root `codegen*` file of GraphQL Codegen
  and a root `serverless.*` file of the Serverless Framework and osls also
  keep the `default` export. A helper module such as `src/lib/serverless.ts`
  stays reportable. cucumber-js reads each named export of `cucumber.js` as a
  profile, and `@vercel/config` reads the named exports of `vercel.ts`, so
  these exports also count as used. Output without `includeEntryExports`
  does not change.

### Added

- **External plugins can opt a config pattern out of the `default` export
  credit.** List the pattern in `configDefaultExportOptOut` when the tool
  reads named exports, or when the pattern also matches ordinary source
  files.
