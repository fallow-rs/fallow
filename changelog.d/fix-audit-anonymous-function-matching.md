### Fixed

- **`fallow audit --gate new-only` no longer reports an unchanged anonymous function as introduced when only its CRAP score changes.** When a file has several anonymous functions, audit matches each head finding to a base finding. Before this fix, a CRAP change from estimated to measured coverage stopped the unchanged function from matching its own base finding. The line-order fallback then paired it with a different, smaller function, and audit reported it as introduced. Audit now also matches on the cyclomatic and cognitive values without the CRAP score before it uses line order. The matched pair still compares the CRAP score. Thanks [@Jerc92](https://github.com/Jerc92) for the report and the reproduction.
  (Closes [#3815](https://github.com/fallow-rs/fallow/issues/3815))
