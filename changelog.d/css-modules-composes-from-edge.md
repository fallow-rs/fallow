### Fixed

- **`fallow dead-code` no longer reports a CSS Modules file as unused when another CSS Modules file composes from it.** A `composes: base rounded from "./shared.module.css"` declaration now adds an import edge to `shared.module.css`, and `base` and `rounded` count as used classes of that file. `composes: name from global` and `composes` without `from` add no edge. The extraction cache version changes, so the first run after the update parses CSS files again.
  (Closes [#4844](https://github.com/fallow-rs/fallow/issues/4844))
