### Fixed

- **A root-only `pnpm install` now gives the type-aware sidecar its TypeScript.** The root `package.json` lists `typescript` 7.0.2 as an exact devDependency. Before, only an override in `pnpm-workspace.yaml` named `typescript`, and no root dependency used it, so the root install had no `typescript`. The lockstep test of the sidecar now checks the devDependency and the override. `docs/development/quality-gates.md` now says that the sidecar `tsrs` backend test needs the sidecar-local install. This change affects contributors only.
  (Closes [#4202](https://github.com/fallow-rs/fallow/issues/4202))
