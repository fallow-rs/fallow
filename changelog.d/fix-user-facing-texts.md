### Fixed

- **`fallow explain` states the facts of each rule correctly.** The
  `unused-class-member` text says that private members are not checked. The
  `unused-file` fix names `entry`, `supportEntry` and `dynamicallyLoaded`.
  `unused-dependency` names `ignoreDependencies`. `mixed-client-server-barrel`
  and `misplaced-directive` list every React Server Components bundler that
  turns them on. `prop-drilling` reports a prop that passes unchanged through
  2 or more components. `unrendered-component` names Vue, Svelte, Astro,
  Angular and Lit.
- **Every structure, styling and coverage rule has an example and a fix.**
  `route-collision`, `dynamic-segment-name-conflict`, `invalid-client-export`,
  `mixed-client-server-barrel`, `misplaced-directive`, `re-export-cycle`,
  `package-cycle`, the five `css-*` rules and the four
  `coverage-intelligence-*` rules had none. The five `runtime-*` rules now
  have one example and one fix each.
- **Help texts name the add-ons as users know them.** The help of the
  runtime coverage options, `coverage setup`, `ci post-check-run --decision`,
  the `type-aware` subcommands, `similar-code status` and the mutation
  options names the runtime coverage add-on, the type-aware companion, the
  similar-code companion and the mutation runner. Help texts lose internal
  labels, all-caps emphasis, contractions, `<https://...>` markup and the
  names of other tools in `--version`. `--fail-on-issues` lists every format
  that exits 0 without it, and the `dupes` defaults name the config file.
- **The config schema describes each key for its users.** `production`
  names the commands that each per-analysis key affects.
  `circularDependencies` names `allow`. `ignoreDependencyOverrides` covers
  npm and bun overrides and bun `resolutions`. `extends` states the remote
  opt-in, the 5 second timeout, the 1 MB limit and the depth limit of 10.
  `ignoreFindings`, `ignoreDependencies`, `ignoreUnresolvedImports`,
  `ignoreExports`, `ignoreCatalogReferences`, `minimumVersion`,
  `includeEntryExports` and `sharedStateAllowlist` gain the facts that a
  user needs, and the override rule keys say what they do. The schema gives
  the defaults of `cache.maxSizeMb`, `effect.schemaExportsAsTypes` and the
  list keys that had none.
- **Messages and descriptions drop internal and history wording.** The error
  for a missing runtime coverage add-on names the add-on. `telemetry status`
  links to the telemetry document instead of a repository path. The glob
  warning no longer compares with an older fallow version. The descriptions
  of the environment variables name the add-ons, the cache trim at 80 and 60
  percent, the 32-snapshot limit of the audit cache and the lookup order of
  `FALLOW_BIN`. Each exit code description ends with a period, and code 2
  describes its JSON error in words.
- **The help of `fallow list [PATH]` says what the path limits.** The path
  limits the listed files, entry points and entry weights. The help said
  that it scoped reported findings, but `list` reports no findings.
- **Runtime coverage messages name the runtime coverage add-on.** The
  signature, protocol, spawn and response errors of the add-on, the
  `FALLOW_COV_BIN` and `FALLOW_COV_BINARY_PATH` errors, the runtime coverage
  fields of the JSON output schema and the `coverage setup` JSON contract
  no longer say "sidecar". `fallow audit-cache prune` reports "lock file"
  and "orphaned marker files" instead of "lock sidecar" and "orphaned
  sidecars". The JSON `reason` value `orphaned-sidecars` stays the same.
- **More descriptions lose all-caps emphasis and internal labels.** The
  `audit` help, the gate descriptions of the JSON output schema, the
  `mixed-client-server-barrel` texts and the opt-in parameters of the
  `inspect_target` MCP tool use plain words. The mutation config keys name
  the mutation runner and the type-aware companion.
- **`fallow schema --cli-reference` shows each value name and an optional value.**
  Options get `value_names`, for example `FROM` and `TO` for `trace --path`,
  and `value_optional`, which is true for `--save-snapshot`.
