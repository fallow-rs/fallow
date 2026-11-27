### Changed

- **`coverage analyze --cloud` keeps the local structure on cloud rows.** When Fallow Cloud sends blast-radius and importance rows without a caller graph, complexity or owners, fallow fills `caller_count`, `caller_count_weighted_by_traffic`, the risk band, `cyclomatic` and `owner_count` from the local AST. The cloud importance score and reason do not change. Before, these fields read as `0`.

- **`coverage analyze --cloud` caps `safe_to_delete` when the local join is incomplete.** When under 80% of the cloud runtime functions match the local AST, each `safe_to_delete` finding becomes `review_required` with medium confidence, and the report adds the `cloud_local_join_incomplete` warning.
  (Refs [fallow-rs/fallow-cloud#328](https://github.com/fallow-rs/fallow-cloud/issues/328))
