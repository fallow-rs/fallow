### Fixed

- Mutation changed scopes reject explicit patches whose paths cannot be placed, instead of substituting a generated Git diff. The CLI exits 2 with a structured error before tests run. Stdin patches and `--diff-file -` preserve their supplied hunks, including an empty scope.
