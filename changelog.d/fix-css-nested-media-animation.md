### Fixed

- **`fallow health --css` credits declarations in nested CSS blocks.** With
  native CSS nesting, these declarations now count:
  - a declaration in an `@media`, `@supports`, `@container`, `@layer`,
    `@scope` or `@starting-style` block inside a style rule;
  - a declaration after a nested rule, as in `.a { &:hover {} color: red; }`.

  A `@keyframes` or a font family that only these declarations use is no
  longer reported as unreferenced. A rule with only such declarations no
  longer counts as empty. The totals of declarations and `!important`
  declarations now include these declarations. They also feed the raw style
  values, the design-token counts and the custom-property definitions, so an
  upgrade can show new findings for them.
  (Closes [#4612](https://github.com/fallow-rs/fallow/issues/4612))
