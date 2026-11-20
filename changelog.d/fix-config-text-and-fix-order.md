### Fixed

- **The description of `ignoreDependencies` states how `*` matches.** The
  text said that a glob uses the `ignorePatterns` syntax, where `*` stops at
  `/`. In a dependency glob, `*` also matches `/`, so `@acme*` covers each
  package in the `@acme` scope.
- **The description of `codeowners` names `--group-by`.** The key also sets
  the CODEOWNERS file for `--group-by owner` and `--group-by section`, not
  only for `fallow health --hotspots --ownership`.
- **The description of `effect.schemaExportsAsTypes` names `HttpApiSchema`.**
  A `pipe` argument can also be a call of an `HttpApiSchema` transform, such
  as `HttpApiSchema.status(201)`.
