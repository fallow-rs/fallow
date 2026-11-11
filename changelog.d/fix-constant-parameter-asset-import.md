### Changed

- **The opt-in `constant-parameter` rule now skips each function of a module that an asset import reads.** An import with a query, such as `./op.ts?raw`, `?url` or `?worker&inline`, or a webpack inline loader request, such as `!!raw-loader!./op.ts`, can copy the module as text into generated code. That code calls the functions with values that the project does not hold, so the rule does not see each caller. A type-only import does not block.
  (Part of [#4491](https://github.com/fallow-rs/fallow/issues/4491))
