### Fixed

- **`fallow mutation` serves a TypeScript mutant behind a project loader that compiles TypeScript.** A loader such as a `module.register()` hook can compile a `.ts`, `.mts` or `.cts` file and give the format `module` or `commonjs`. The overlay hook of the runner now strips the types of the mutant before it serves the mutant in that format. Before, the child process loaded raw TypeScript as JavaScript and failed with a `SyntaxError`. When type stripping does not accept the mutant (for example an `enum`), the run records an overlay error that names the file.
  (Closes [#4172](https://github.com/fallow-rs/fallow/issues/4172))
