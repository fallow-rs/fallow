### Fixed

- **`fallow mutation` serves a TypeScript mutant behind a project loader that compiles TypeScript.** A loader such as a `module.register()` hook can compile a `.ts`, `.mts` or `.cts` file and give the format `module` or `commonjs`. The overlay hook of the runner now strips the types of the mutant before it serves the mutant in that format. Before, the child process loaded raw TypeScript as JavaScript and failed with a `SyntaxError`. The run records an overlay error that names the file in two cases: type stripping does not accept the mutant (for example an `enum`), or the loader gives `commonjs` but the mutant keeps `import` or `export` syntax.
  (Closes [#4172](https://github.com/fallow-rs/fallow/issues/4172))
