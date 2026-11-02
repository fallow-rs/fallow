### Added

- **A `graphql-config` plugin keeps GraphQL schema and operation files used.** GraphQL tools read a `graphql.config.*`, `.graphqlrc` or `.graphqlrc.*` file, or the `graphql` key in `package.json`. The plugin reads the `schema` and `documents` globs of that file, also under `projects.<name>`, and marks the `.graphql`, `.gql` and `.graphqls` files that they match as used. A glob is relative to the directory of the config file, so a config in a workspace package works. The config file itself is also marked as used. The plugin activates from the `graphql-config` package, from a config file anywhere in the project, or from a `graphql` object in `package.json`. A glob that climbs out of a workspace package, such as `../../shared/*.graphql`, also works. A URL, a negated glob, and a glob for source files such as `src/**/*.tsx` get no credit.

### Fixed

- **`fallow dead-code` no longer reports `.pnpmfile.cjs` or `.pnpmfile.mjs` as unused files.** The pnpm plugin did not activate from `pnpm-workspace.yaml` or `pnpm-lock.yaml`, so its always-used files had no effect. The plugin now activates from these files, from a pnpmfile, and from a `packageManager` field that names pnpm. It also marks the files that the `pnpmfile` setting names in `pnpm-workspace.yaml` or `.npmrc` as used.
