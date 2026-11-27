### Changed

- **`coverage analyze --cloud` keeps hot-path, blast-radius and importance IDs on a line move.** The IDs hash the file and the function name, or the `stable_id` when the file has more than one function of that name. This is the recipe of the Fallow Cloud rows, so an edit above a function no longer gives it a new ID. Each entry also carries `legacy_id` with the old file+line ID for a transition window. Match a stored ID against `id` or `legacy_id`. Cloud rows pass their `legacy_id` through.
  (Refs [fallow-rs/fallow-cloud#1597](https://github.com/fallow-rs/fallow-cloud/issues/1597))
