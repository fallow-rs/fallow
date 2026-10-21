### Fixed

- **A `fallow mutation` run with `--time-budget` now keeps time to confirm
  its survivors.** Before, the first pass could use the full budget. Then no
  survivor or timeout got its retry, the result cache stored none of them,
  and a repeat run with the same budget ran the same mutants again. Now the
  first pass stops new mutant runs early by the estimated retry time of the
  survivors and timeouts that it found. The confirmed results go into the
  cache, and a repeat run continues with the next mutants. A survivor that
  gets no retry stays unconfirmed and is not stored.
  (Closes [#3665](https://github.com/fallow-rs/fallow/issues/3665))
