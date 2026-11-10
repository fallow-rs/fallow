### Fixed

- **`fallow mutation` with Vitest now shows a `vi.stubEnv` value to code that reads `import.meta.env`.** Before, such a test passed with `vitest run` but failed in the baseline, and the run was `partial` with the reason `baseline-failure`. The cause was the order of the transforms. The companion replaced the code after the transforms of Vitest ran, so the `import.meta.env` read did not go to the environment object of Vitest. Now the companion gives the instrumented file to Vite as the file on disk. Each transform of Vite, Vitest and the project then runs on the instrumented code, as with `vitest run`.
  (Closes [#4470](https://github.com/fallow-rs/fallow/issues/4470))
