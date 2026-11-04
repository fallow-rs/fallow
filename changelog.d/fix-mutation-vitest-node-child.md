### Fixed

- **`fallow mutation` with Vitest now gives the active mutant to Node child processes.** A test can start a Node process (`spawn`, `exec`, `execFile` or `fork`) that imports a source file. Before this fix, that process loaded the original file, so an assertion on its output could not kill the mutant and the report showed a false survivor. The Vitest setup file now adds the subprocess preload to `NODE_OPTIONS` with the active mutant, as the `node:test` adapter does. Each run also checks the integrity log of the overlay, so an ambiguous copied source reports `overlay-incomplete`.
  (Closes [#4049](https://github.com/fallow-rs/fallow/issues/4049))
