### Fixed

- **`fallow audit` no longer reports an old clone group as introduced when the change rewrites the lines above one copy.**
  The audit maps each head copy of a clone back to its base lines. When the
  first or last line of a copy was a rewritten line that replaced several base
  lines, the mapped range grew to the far end of the replaced lines and fell
  outside the base clone. A rename of the other copy, together with fewer
  import lines above this copy, could then fail `--gate new-only` on
  duplication that the change did not write. The rewritten edge line now maps
  to the replaced base line next to the unchanged lines of the copy.
  (Closes [#4580](https://github.com/fallow-rs/fallow/issues/4580))
