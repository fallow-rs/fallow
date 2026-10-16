### Changed

- **A single `*` now stays within one path segment in every config path
  glob.** If your config uses a single `*` to match nested files, change it
  to `**`. For example, change `src/generated/*` to `src/generated/**`, and
  `*.test.ts` to `**/*.test.ts`. For each pattern that now matches fewer
  files, fallow prints a warning on stderr. The warning names the config
  field, the pattern, the count of files that only the old rule matched, and
  the rewrite. `--quiet` hides the warning, and the JSON output does not
  change. The `entry` and `dynamicallyLoaded` fields already use this rule.
  It now also applies to `ignorePatterns`, `ignoreFindings`,
  `overrides[].files`, `ignoreExports[].file`,
  `ignoreCatalogReferences[].consumer`, `duplicates.ignore`,
  `similarCode.ignore`, `health.ignore`, `health.thresholdOverrides[].files`,
  the `boundaries` zone and coverage globs, the rule-pack file globs, and the
  `framework[]` fields `usedExports[].pattern`, `manifestEntries[].manifests`
  and `detection.pattern`. Only `**` crosses directories, as in picomatch.
  Before this change, `ignorePatterns: ["src/generated/*"]` also ignored the
  files in the subfolders of `src/generated`, and `"files": ["*.test.ts"]` in
  `overrides` matched test files at any depth. A narrower glob can show new
  findings. A line that you copy from `.gitignore` can match fewer files in
  `ignorePatterns`. Fallow matches the glob against each file path, not
  against each directory. Thus `src/generated/*` in `.gitignore` ignores the
  subfolders of `src/generated`, but in `ignorePatterns` it does not. Globs
  that match package names or import specifiers, such as
  `ignoreDependencies` and `ignoreUnresolvedImports`, do not change.
