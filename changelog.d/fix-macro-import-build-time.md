### Fixed

- **A macro import is build time.** An import with `with { type: "macro" }` runs its target in the bundler, and the output holds only the result. A devDependency that only a macro module imports, or that a macro import runs directly, is no longer a devDependency in production. The macro module and its exports stay used. A module that a runtime import also reaches stays production code. The extraction cache version moves because extraction now records macro imports.
