### Fixed

- **A member read on a local with the name of a namespace import no longer uses the export of that name.** In `catch (e) { e.message }` after `import * as e from './errors'`, the export `message` was reported as used. The same was true for a binding in a static block, and for the forms `e['message']`, `e.message.length`, `e().message` and `e[0].message`. Now a member read uses an export of the import only when its root resolves to the import binding.
  (Closes [#5231](https://github.com/fallow-rs/fallow/issues/5231))
