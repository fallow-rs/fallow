### Changed

- **The type-aware companion protocol changes to version 10. Install `fallow-type-aware` at the same version as `fallow`.** A companion of an older release does not keep checker reads inside the project, so this release refuses it. What you see depends on how you install the companion:
  - npm, pnpm or Yarn: the `fallow` launcher uses only a `fallow-type-aware` package of the same version. A pinned older version is not used, so Fallow finds no companion. Upgrade both packages together, for example `npm install --save-dev fallow@<version> fallow-type-aware@<version>`.
  - `fallow type-aware setup`: Fallow uses an older install only when it speaks the same protocol, so it skips the installs of earlier releases. Run `fallow type-aware setup` again, then `fallow type-aware cleanup` to remove the old installs.
  - `FALLOW_TYPE_AWARE_BIN` that points to an older companion: the companion stops with `unsupported protocol_version`, and Fallow adds the command to run.

  The GitHub Action and the GitLab template install the matching companion themselves. While the companion does not match, the type-aware pass does not run. With the default `typeAware.require: best-effort`, the run reports the syntactic findings. With `complete`, the run exits with code 2.

### Security

- **Fallow reads project files only inside the project scope.** Each reader below the project root now reads a file only when it resolves to a regular file inside the root, with a size limit. A file or a link that leads out of the root counts as missing. This applies to SARIF snippets, CI suggestions, the diff-risk rules of `fallow audit`, `fallow migrate`, baselines, snapshots, saved mutation reports, coverage inputs, V8 scripts, plugin config probes, CI workflow, hook and infrastructure readers. A path that you name outside the project stays allowed.
- **A cached graph is used only when its structure is valid.** Fallow rebuilds the graph when the cache has a module list that differs from the discovered files, or an index that is out of bounds.
- **The input files of `fallow ci`, the build directory of `coverage upload-source-maps` and the vendor directory of `ci-template gitlab --vendor` must not lead out of the working directory or the repository through a link.**
- **The type-aware companion reads files only inside the analysis root**, inside a `node_modules` directory at or above it, and inside the compiler package. A project config that leads out of the root is dropped. A semantic response path that leads above the root is refused.
- **External plugin globs stay inside the project on every route**, also for the coverage upload. `fallow plugin-check` no longer evaluates the detection of a plugin that fails validation.
