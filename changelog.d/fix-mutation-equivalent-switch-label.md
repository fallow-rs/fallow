### Fixed

- **`fallow mutation` no longer makes an equivalent mutant for a `case`
  label whose body is the same as the `default` body.** The original label
  and the empty-string or swapped-boolean label select the same body for
  each input, so no test can kill the mutant. The rule applies only when all
  labels are literals, both bodies end in an unconditional exit, and no other
  label has the original or the mutated value.
  (Closes [#3639](https://github.com/fallow-rs/fallow/issues/3639))
