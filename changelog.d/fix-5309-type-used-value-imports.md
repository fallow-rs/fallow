### Fixed

- **`circular-dependency` no longer reports a cycle that closes only through imports that a file uses as types or not at all.** TypeScript removes a plain `import { X }` when the file never reads `X` as a value, also when `X` is a class. Such an import does not load the target at runtime, so it no longer closes a cycle. The import still counts in three cases: when the file reads the binding as a value, when the importer is a `.tsx` file (the binding can be the JSX factory), and in a file with a decorator when the tsconfig sets `emitDecoratorMetadata`. The tsconfig options that keep each import (`verbatimModuleSyntax`, `preserveValueImports`) still apply.
  (Closes [#5309](https://github.com/fallow-rs/fallow/issues/5309))
