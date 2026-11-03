### Changed

- **`fallow-report-protocol` now holds the generic gate map `GateOutcomes<K>`.** Each tool supplies its own gate name type. `fallow-output` uses `GateOutcomes<GateName>`, so the JSON of `gate_outcomes` does not change. The doc comments of the shared report types no longer name fallow commands, and `docs/output-schema.json` changes only in descriptions. The `GateOutcomes` description still lists every gate name. The CLI now reads the exit codes 1 and 2 from the shared constants, and a test rejects a new literal.
  (Refs [#4081](https://github.com/fallow-rs/fallow/issues/4081))
