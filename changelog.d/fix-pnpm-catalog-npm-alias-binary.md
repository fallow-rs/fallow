### Fixed

- **`fallow dead-code` no longer reports an npm alias from a catalog as an unused dependency when a script runs its binary.** A manifest can declare `"@typescript/native": "catalog:"` with the catalog entry `npm:typescript@~7.0.2`. Fallow now reads the catalog spec, so a script that runs `tsc` credits `@typescript/native`, as it does for the inline `npm:` spec. This works for the default catalog and for named catalogs in `pnpm-workspace.yaml` and in a Bun root `package.json`. A catalog alias next to a plain declaration of the same package stays reported, because the plain declaration provides the binary.
  (Closes [#4258](https://github.com/fallow-rs/fallow/issues/4258))
