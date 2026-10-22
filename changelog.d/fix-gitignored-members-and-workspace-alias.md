### Fixed

- **`fallow dead-code` no longer reports the dependencies of a gitignored workspace member.** A pnpm workspace can declare a directory that CI fills with a clone of an upstream repository, for example `vite` or `rolldown`. Source discovery does not read the files of a gitignored member, so the run cannot prove that its dependencies are unused. The member stays a workspace for imports and catalog references.
- **A `workspace:<name>@<range>` dependency alias resolves to the aliased workspace package.** With `"vite": "workspace:@scope/core@*"`, an import of `vite/pack` resolves through the `exports` map of `@scope/core`. Before, a workspace member with the name `vite` took the import, and the import was unresolved when that member had no `./pack` export.
