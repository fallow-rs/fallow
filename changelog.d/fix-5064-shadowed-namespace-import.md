### Fixed

- **A local with the name of a namespace import no longer hides the unused exports of the imported module.** In `import * as e from "./errors"`, a use of a local `e` (for example `String(e)` in `catch (e)`) counted as a use of the whole namespace. Fallow then marked each export of `./errors` as used. Now only a use that resolves to the import binding gives this credit. The fix also covers a binding in a static block and the name of a named function or class expression.
  (Closes [#5064](https://github.com/fallow-rs/fallow/issues/5064))
