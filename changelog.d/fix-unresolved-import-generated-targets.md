### Fixed

- **`unresolved-import` no longer reports two kinds of imports of generated files.**
  - An import of a file that the repository ignores, when the import leaves out the extension. fallow already skipped a relative import whose missing target path a `.gitignore` rule ignores. The check now also tests the file names that the resolver tries: each module extension, each directory index, and the `.ts` source of a `.js` specifier. Thus `./keys.generated` is not reported when the rule names `keys.generated.ts`.
  - A type-only import of the React Router or Remix route types of a parent route, such as `../+types/layout`. The framework writes a `+types` directory beside each route module, and fallow already skipped `./+types/` imports. A runtime import of `+types` stays reported.
