### Fixed

- **Commands in a development container file and in a `t3.json` project file now count as uses.** `fallow dead-code` reads the lifecycle commands of `.devcontainer/devcontainer.json`, `.devcontainer.json` and `.devcontainer/<name>/devcontainer.json` (`initializeCommand`, `onCreateCommand`, `updateContentCommand`, `postCreateCommand`, `postStartCommand`, `postAttachCommand`), and the `scripts[].command` values of a `t3.json` file. It reads each command as it reads a package.json script: a script file that the command runs is an entry point, a binary that it runs credits the package, and a project shell file that it runs is read too.
  (Refs [#4912](https://github.com/fallow-rs/fallow/issues/4912))
- **A CI step that puts a `node_modules/.bin` path in a shell variable now credits the package.** A step such as `bundler=scripts/node_modules/.bin/esbuild` followed by `"$bundler" src/a.ts` runs the binary through the variable, so fallow reported the package as an unused dependency. The assignment now counts as the use of the binary.
  (Refs [#4912](https://github.com/fallow-rs/fallow/issues/4912))
