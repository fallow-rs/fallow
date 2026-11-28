### Fixed

- **`fallow dupes` tokenizes type annotations on variable declarations.** Declarations like `let value: Alpha<One>;` and `let value: Beta<Two>;` previously emitted identical token sequences because `VariableDeclarator::type_annotation` was not visited, allowing clone ranges to start on a line whose type annotation differed. Variable declaration type annotations now emit a colon and type tokens just like parameter annotations, while `crossLanguage` still strips them. The duplication token cache version advances from 27 to 28, so the token cache rebuilds once.
  (Closes [#5435](https://github.com/fallow-rs/fallow/issues/5435))
