### Fixed

- **An older fallow no longer drops data from an Impact store that a newer
  fallow wrote.** A project-local fallow could rewrite the shared store through
  its own older model, and fields it did not know were lost. Every write path
  now leaves such a store unchanged. `fallow impact enable`, `fallow impact
  disable`, and `fallow init --decline` exit with code 2 and name the store
  schema and the fallow version of the newest record. Audit and combined runs do not
  record into it. Reads stay available.
  (Closes [#3597](https://github.com/fallow-rs/fallow/issues/3597))
