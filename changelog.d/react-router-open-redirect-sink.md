### Added

- **`fallow security` reports a non-literal target of React Router `redirect`, `redirectDocument` and `replace`.** The new `react-router-open-redirect` category (CWE-601) works like `nextjs-open-redirect`. It is active when the project declares `react-router` or `@react-router/dev`. The callee must be the binding of an import from `react-router`, so a local function named `redirect` or `replace` gives no candidate. Thanks to @cr101 for the report and the reproduction.
  (Closes [#4951](https://github.com/fallow-rs/fallow/issues/4951))
