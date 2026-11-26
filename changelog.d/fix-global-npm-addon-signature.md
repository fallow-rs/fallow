### Fixed

- **A global install of the runtime coverage add-on now runs.** After `npm install -g @fallow-cli/fallow-cov`, the `PATH` search found the Node launcher of the package. The launcher has no `.sig` file, so fallow stopped with exit code 4. The `PATH` search now finds the signed binary of the platform package of the same install. This works for global installs with npm, pnpm, Yarn and Bun, and for the install that `fallow coverage setup` makes. The signature check still runs on the binary that fallow starts. When fallow cannot find the signed binary, the error now tells you to set `FALLOW_COV_BIN`.
  (Closes [#5303](https://github.com/fallow-rs/fallow/issues/5303))
