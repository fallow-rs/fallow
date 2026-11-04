### Fixed

- **The VS Code sidebar keeps an open group open after a refresh.** Before, a
  group such as `Unused Component Inputs (13)` closed when a refresh changed
  the count in its label. Each group in the Unused Code, Health, Security and
  Runtime Coverage views now has a stable id, so VS Code keeps the expansion
  state.
  (Closes [#4090](https://github.com/fallow-rs/fallow/issues/4090))
