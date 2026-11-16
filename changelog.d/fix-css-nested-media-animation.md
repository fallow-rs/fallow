### Fixed

- **`fallow health --css` credits an animation in a nested at-rule.** A
  declaration inside an `@media`, `@supports` or `@container` block that is
  nested in a style rule (native CSS nesting) now counts. A `@keyframes` that
  only such a block uses is no longer reported as unreferenced. The totals of
  declarations and `!important` declarations now also include these blocks.
  (Closes [#4612](https://github.com/fallow-rs/fallow/issues/4612))
