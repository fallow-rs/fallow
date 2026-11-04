### Changed

- **Contributors set up a clone with one command, `pnpm bootstrap`.** The root repository tools now install with pnpm 11.25.0, the same pnpm as the VS Code extension, and `pnpm-lock.yaml` replaces the root `package-lock.json` with the same dependency versions. `pnpm bootstrap` checks for Node.js, pnpm and hk, removes a stale `core.hooksPath=.githooks` setting, installs the hk hooks, and installs the root and extension dependencies. `mise.toml` now pins pnpm. This change affects contributors only. The released packages do not change.
