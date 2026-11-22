### Fixed

- **The GitHub Action finds a type-aware config through `extends` and parent folders.**
  With `type-aware: auto`, the Action installed the sidecar only when the first config file in the root enabled `typeAware`. It now asks the installed fallow CLI for the resolved config, so a setting in an `extends` base or in a parent config also counts. A fallow version that cannot print the resolved config falls back to the old file check.
- **The GitHub Action starts the type-aware sidecar on Windows.**
  The Action set `FALLOW_TYPE_AWARE_BIN` to the `.mjs` file of the sidecar, which Windows cannot start as a program. On Windows it now sets `FALLOW_TYPE_AWARE_BIN` to Node and `FALLOW_TYPE_AWARE_SCRIPT` to the sidecar file, as the npm package of fallow does.
