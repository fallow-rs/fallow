### Added

- **Unused exports in generated files and in shadcn/ui folders now name the config entry that hides them.** Fallow keeps these findings in the report. In the human report, the "Unused exports" section prints one hint for each group of 3 findings or more, with the `ignoreExports` entry to add: one pattern for the generated files, and one for each shadcn/ui folder that a `components.json` names. In JSON, each such finding has an `add-to-config` action with the `ignoreExports` rule. An unused export or unused type in a generated file has `auto_fixable: false` on its `remove-export` action and no `suppress-line` action, because the generator writes the file again and removes the edit or the comment.
  (Refs [#4912](https://github.com/fallow-rs/fallow/issues/4912))
