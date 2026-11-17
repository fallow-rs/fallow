### Fixed

- **`fallow mutation` now treats an `AUTO-GENERATED ... DO NOT EDIT` header as a generated file.** The plan prunes the mutants of such a file, as it already did for a `Code generated ... DO NOT EDIT` header.
  (Closes [#4736](https://github.com/fallow-rs/fallow/issues/4736))
- **`fallow mutation <path>` now names the reason when a path selects no file.** Before, a path that `ignorePatterns` excluded gave the message that the project had no test files. The human output now prints one line for each unmatched path, with its reason, and the JSON output adds `unmatched_path_reasons`. The reason `ignored` applies only when the ignore set excludes a source file that the plan would otherwise select.
  (Closes [#4739](https://github.com/fallow-rs/fallow/issues/4739))
