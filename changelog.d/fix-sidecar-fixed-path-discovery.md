### Fixed

- **Runtime coverage finds the sidecar in a Yarn Plug'n'Play project.** Fallow now reads the signed binary from `.yarn/unplugged`. Before, discovery stopped at the unsigned wrapper, and the run failed with a signature error.

- **An unsigned `node_modules/.bin/fallow-cov` wrapper no longer hides a working sidecar.** Discovery now continues to `~/.fallow/bin` and `PATH`. It uses the wrapper only when no other location has a sidecar, so the signature error names the file that exists. Before, a pnpm project with a custom `virtualStoreDir` failed with a signature error even when `~/.fallow/bin/fallow-cov` was installed.

### Security

- **Runtime coverage finds the `fallow-cov` sidecar in fixed install locations only.** Fallow looks in the `@fallow-cli/fallow-cov-<platform>` package in `node_modules`, the pnpm and Bun stores, the Yarn `unplugged` folder, `node_modules/.bin`, `~/.fallow/bin` and `PATH`. This applies to `health`, `audit` and `security` with `--runtime-coverage`, to the MCP runtime-coverage tools, and to the sidecar check of `fallow coverage setup`.
