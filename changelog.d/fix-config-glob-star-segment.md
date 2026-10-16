### Changed

- **A single `*` now stays within one path segment in every config path
  glob.** The `entry` and `dynamicallyLoaded` fields already use this rule.
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
  findings. To keep the old match, change the single `*` to `**`, for example
  `src/generated/**` or `**/*.test.ts`. A line that you copy from
  `.gitignore` can match fewer files in `ignorePatterns`. Fallow matches the
  glob against each file path, not against each directory. Thus
  `src/generated/*` in `.gitignore` ignores the subfolders of
  `src/generated`, but in `ignorePatterns` it does not. Use
  `src/generated/**` in `ignorePatterns` to ignore the subfolders too. Globs
  that match package names or import specifiers, such as
  `ignoreDependencies` and `ignoreUnresolvedImports`, do not change.
