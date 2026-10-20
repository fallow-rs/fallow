### Fixed

- **The `default` export of a tool config file is no longer reported as an
  unused export under `includeEntryExports`.** The tool loads its config file
  and reads the default export. This rule applies to every config pattern of
  an active plugin, built-in or external, for example
  `tailwind.config.js`, `jest.config.js`, `playwright.config.ts` and a config
  file in a workspace package. Named exports of a config file stay
  reportable. Some patterns do not get the credit: Gatsby `gatsby-node`,
  `gatsby-browser` and `gatsby-ssr` (Gatsby reads named exports), and GraphQL
  Codegen `codegen*`, Serverless `serverless.*` and Vercel `vercel.*`, because
  these patterns also match ordinary source files. Output without
  `includeEntryExports` does not change. Thanks to
  [@osazemeu](https://github.com/osazemeu) for the report.
  (Closes [#3691](https://github.com/fallow-rs/fallow/issues/3691))
