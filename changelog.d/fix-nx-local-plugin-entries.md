### Fixed

- **Nx local executors, generators and plugins are no longer unused files.** Fallow now reads each `executors.json` and `generators.json` collection. The `implementation`, `batchImplementation`, `factory` and `hasher` files of each entry are entry points, and each `schema` file counts as used. A local plugin path in the `plugins` array of `nx.json` is an entry point, together with the `main` file of its `package.json`. For a local executor path such as `./tools/plugin:build`, fallow reads the collections that the `package.json` of that folder names. Fallow also no longer credits a local executor path as a package name.
  (Closes [#5201](https://github.com/fallow-rs/fallow/issues/5201))
