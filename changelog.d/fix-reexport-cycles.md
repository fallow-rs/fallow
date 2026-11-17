### Fixed

- **`fallow dead-code` no longer reports `export * as Ns from "./self"` as a re-export self-loop.** A namespace re-export adds one named export and forwards no names, so chain propagation cannot loop through it. Fallow now keeps these edges out of the re-export cycle check, also for loops between two or more files. The `Re-export self-loop detected` warning for this form is gone too. (Closes [#4722](https://github.com/fallow-rs/fallow/issues/4722))
