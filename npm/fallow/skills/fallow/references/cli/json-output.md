# JSON Output Structure

## `dead-code` output

```json
{
  "kind": "dead-code",
  "schema_version": 7,
  "version": "3.49.0",
  "elapsed_ms": 45,
  "total_issues": 12,
  "entry_points": {
    "total": 5,
    "sources": { "package_json_scripts": 2, "next_js": 3 }
  },
  "summary": {
    "total_issues": 12,
    "unused_files": 1,
    "unused_exports": 1,
    "unused_types": 1,
    "unused_dependencies": 1,
    "unused_enum_members": 0,
    "unused_class_members": 0,
    "unresolved_imports": 0,
    "unlisted_dependencies": 0,
    "duplicate_exports": 0,
    "type_only_dependencies": 0,
    "test_only_dependencies": 0,
    "circular_dependencies": 0,
    "re_export_cycles": 0,
    "boundary_violations": 0,
    "stale_suppressions": 0
  },
  "unused_files": [{ "path": "src/old.ts" }],
  "unused_exports": [{ "path": "src/utils.ts", "name": "unusedFn", "line": 42, "actions": [{"type": "remove-export", "auto_fixable": true, "description": "Remove the unused export from the public API"}, {"type": "suppress-line", "auto_fixable": false, "description": "Suppress with an inline comment above the line", "comment": "// fallow-ignore-next-line unused-export"}] }],
  "unused_types": [{ "path": "src/types.ts", "name": "OldType", "line": 10 }],
  "unused_dependencies": [{ "name": "lodash", "line": 5, "used_in_workspaces": ["packages/web"] }],
  "unused_dev_dependencies": [{ "name": "jest", "line": 8 }],
  "unused_enum_members": [{ "path": "src/enums.ts", "enum_name": "Status", "member": "Archived", "line": 5 }],
  "unused_class_members": [{ "path": "src/service.ts", "class_name": "Service", "member": "oldMethod", "line": 20 }],
  "unresolved_imports": [{ "path": "src/index.ts", "specifier": "./missing", "line": 3 }],
  "unlisted_dependencies": [{ "name": "chalk", "imported_from": [{ "path": "src/cli.ts", "line": 1, "col": 0 }] }],
  "duplicate_exports": [{ "name": "Config", "locations": ["src/config.ts:5", "src/types.ts:12"] }],
  "circular_dependencies": [{ "cycle": ["src/a.ts", "src/b.ts", "src/a.ts"], "line": 3, "col": 0, "is_cross_package": false }],
  "re_export_cycles": [{ "files": ["src/api/index.ts", "src/api/internal/index.ts"], "kind": "multi-node", "actions": [{ "type": "fix", "kind": "refactor-re-export-cycle", "auto_fixable": false, "description": "Remove one `export * from` (or `export { ... } from`) statement on any one member to break the cycle" }, { "type": "suppress-file", "kind": "suppress-file", "auto_fixable": false, "comment": "// fallow-ignore-file re-export-cycle" }] }],
  "boundary_violations": [{ "from_path": "src/ui/Button.ts", "to_path": "src/data/db.ts", "from_zone": "ui", "to_zone": "data", "import_specifier": "../data/db", "line": 5, "col": 0 }],
  "unused_optional_dependencies": [{ "name": "fsevents" }],
  "type_only_dependencies": [{ "name": "zod", "used_in": ["src/schema.ts"], "line": 12 }],
  "test_only_dependencies": [{ "name": "msw", "path": "package.json", "line": 15 }],
  "stale_suppressions": [{ "path": "src/utils.ts", "line": 5, "col": 0, "origin": { "type": "inline_comment", "issue_type": "unused-export", "is_file_level": false } }]
}
```

For dependency findings, `used_in_workspaces` means the package is imported by another workspace even though the declaring workspace does not import it. Move the dependency to the consuming workspace instead of auto-removing it.

### `actions` Array

Every issue in `dead-code` JSON output includes an `actions` array with structured fix suggestions. Each action has:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | yes | Action type in kebab-case (for example `remove-export`, `remove-file`, `remove-dependency`, `move-dependency`, `suppress-line`, `add-to-config`) |
| `auto_fixable` | bool | yes | `true` if `fallow fix` handles this action automatically. Evaluated PER FINDING, not per action type: the same `type` may carry `true` on one finding and `false` on another when per-instance guards in the applier discriminate. Filter on this bool of each individual action, not on `type` alone. |
| `description` | string | yes | Human-readable description of the action |
| `comment` | string | no | Suppression comment text (on `suppress-line` actions) |
| `note` | string | no | Additional context on non-auto-fixable items |
| `config_key` | string | no | Config field to update (on `add-to-config` actions) |
| `value` | string \| array | no | Value to add to the config field (on `add-to-config` actions). Scalar for `ignoreDependencies`-style keys (e.g. `"lodash"`); array of `{ file, exports }` rule objects for `ignoreExports`. |
| `value_schema` | string | no | URL pointing at the JSON Schema fragment that describes `value` (on `add-to-config` actions). Agents that want to validate `value` before writing it into a user's config can fetch and apply the linked schema. |

Example:

```json
{
  "path": "src/utils.ts",
  "name": "helperFn",
  "line": 10,
  "actions": [
    {
      "type": "remove-export",
      "auto_fixable": true,
      "description": "Remove the unused export from the public API"
    },
    {
      "type": "suppress-line",
      "auto_fixable": false,
      "description": "Suppress with an inline comment above the line",
      "comment": "// fallow-ignore-next-line unused-export"
    }
  ]
}
```

Dependency issues use `add-to-config` with `config_key` and `value`:

```json
{
  "name": "autoprefixer",
  "line": 5,
  "actions": [
    {
      "type": "remove-dependency",
      "auto_fixable": true,
      "description": "Remove from package.json dependencies"
    },
    {
      "type": "add-to-config",
      "auto_fixable": false,
      "description": "Add to ignoreDependencies in fallow config",
      "config_key": "ignoreDependencies",
      "value": "autoprefixer",
      "value_schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json#/properties/ignoreDependencies/items"
    }
  ]
}
```

When a dependency action is `move-dependency`, `auto_fixable` is `false`; the package is imported from another workspace and needs a package.json ownership move rather than removal.

Per-instance `auto_fixable` flips today (the same action `type` flipping between findings):

- `remove-catalog-entry` (unused-catalog-entries): `true` only when `hardcoded_consumers` is empty; `false` otherwise (the applier skips the entry to avoid breaking `pnpm install`).
- `remove-dependency` vs `move-dependency` (dependency findings): primary action flips between `remove-dependency` (`true`) and `move-dependency` (`false`) on `used_in_workspaces`.
- `add-to-config` for `ignoreExports` (duplicate-exports): `true` when `fallow fix` can safely apply the action, which today means EITHER a fallow config file already exists OR no config exists and the working directory is NOT inside a monorepo subpackage. In the second case the applier creates `.fallowrc.json` using `fallow init`'s framework-aware scaffolding and layers the new rules on top. `false` inside a monorepo subpackage with no workspace-root config (the applier refuses to fragment per-package configs). Pass `--no-create-config` to `fallow fix` from pre-commit hooks, CI bots, and `fallow watch` to opt out of the create-fallback; the action then surfaces with `auto_fixable: false`.
- `update-catalog-reference` (unresolved-catalog-references): always `false` today; non-singleton on the wire so a future applier can promote it without a schema change.
- All `suppress-line` and `suppress-file` actions are uniformly `false`.

### Health `actions` array (CRAP findings)

Health findings (`fallow health` JSON output) include an `actions` array. Primary action selection is formula-aware: the rule first checks whether full coverage CAN bring CRAP under threshold (CRAP bottoms out at `cyclomatic` at 100% coverage, so `cyclomatic < maxCrap` means coverage is a viable remediation), then uses `coverage_tier` to choose the description.

| Condition | Primary action |
|-----------|----------------|
| `cyclomatic >= maxCrap` (coverage cannot remediate, regardless of tier) | `refactor-function` |
| `cyclomatic < maxCrap` and `coverage_tier=none` | `add-tests` ("start from scratch") |
| `cyclomatic < maxCrap` and `coverage_tier=partial` or `high` | `increase-coverage` ("targeted branch coverage") |
| Cyclomatic/cognitive triggered (no CRAP) | `refactor-function` |

The `coverage_tier` field is `"none"` (file not test-reachable / Istanbul 0%), `"partial"` (Istanbul `(0, 70)` / estimated 40%), or `"high"` (Istanbul `>= 70` / estimated 85%).

Each CRAP finding also carries a `coverage_source` discriminator: `"istanbul"` (direct fnMap match for this function, from an Istanbul map or from raw V8 coverage; `summary.coverage_input_format` names which), `"estimated"` (graph-based estimate evaluated against the finding's own file), or `"estimated_component_inherited"` (graph-based estimate inherited from an Angular component `.ts` reached via the inverse `templateUrl` edge). The report summary carries `coverage_source_consistency` (`"uniform"` or `"mixed"`) whenever emitted CRAP findings have source data; grouped health JSON also includes `groups[].coverage_source_consistency`. Synthetic `<template>` findings on Angular `.html` templates use the `estimated_component_inherited` source and include an `inherited_from` field with the project-relative path to the owning `.component.ts`. When the inherit path applies, the primary `increase-coverage` action targets that `.ts` file (description names the component path explicitly and includes a `target_path` field) so AI agents add component tests rather than scaffolding tests against a structurally untestable `.html` path. The human `fallow health` output renders `(inherited from <project-relative-path>.component.ts)` after the CRAP score on those rows (project-relative since fallow 2.78.0; was the bare basename before). This is the JIT-test fallback (Angular's runtime renders templates via `ɵɵconditional` / `ɵɵrepeaterCreate` calls; Istanbul never has `fnMap` entries keyed at `.html` paths). AOT-compiled coverage with source-map back-mapping is planned as a phase 2 follow-up; when it lands, `coverage_source` will gain a `"measured_aot_source_map"` variant.

When CRAP-only with cyclomatic count within `health.crapRefactorBand` of `maxCyclomatic` AND cognitive at or above `maxCognitive / 2`, a secondary `refactor-function` is appended. The default band is `5`; set it to `0` to only add the secondary refactor after cyclomatic reaches `maxCyclomatic`. The cognitive floor suppresses false positives on flat type-tag dispatchers and JSX render maps (high CC, near-zero cog). A single finding can carry multiple action types: e.g. a finding that exceeds both cyclomatic and CRAP at `coverage_tier=partial` gets `increase-coverage` AND `refactor-function`. Treat the first non-`suppress-line` action as primary.

The `suppress-line` action is auto-omitted when `--baseline`/`--save-baseline` is set, OR when `health.suggestInlineSuppression: false` in config. The report root carries an `actions_meta: { suppression_hints_omitted: true, reason: "baseline-active" | "config-disabled" }` breadcrumb in that case.

### `baseline_deltas` Object

When `--baseline` is used in combined output, the JSON includes a `baseline_deltas` object showing per-category changes since the baseline:

```json
{
  "baseline_deltas": {
    "total_delta": -3,
    "per_category": {
      "unused_files": { "current": 5, "baseline": 7, "delta": -2 },
      "unused_exports": { "current": 10, "baseline": 11, "delta": -1 }
    }
  }
}
```

## `dupes` output

```json
{
  "kind": "dupes",
  "schema_version": 7,
  "version": "3.49.0",
  "elapsed_ms": 82,
  "total_clones": 15,
  "total_lines_duplicated": 230,
  "duplication_percentage": 4.2,
  "clone_groups": [
    {
      "instances": [
        { "path": "src/a.ts", "start_line": 10, "end_line": 25 },
        { "path": "src/b.ts", "start_line": 40, "end_line": 55 }
      ],
      "tokens": 120,
      "lines": 16,
      "family": { "suggestion": "extract_function", "shared_files": ["src/a.ts", "src/b.ts"] }
    }
  ],
  "mirrored_directories": [
    { "dir_a": "src/components", "dir_b": "src/legacy/components", "shared_clones": 4 }
  ]
}
```

The `mirrored_directories` array identifies directory pairs that share many clone groups, suggesting structural duplication (e.g., a copy-pasted module that was never cleaned up).

## `fix` output (dry-run)

```json
{
  "changes": [
    { "path": "src/utils.ts", "action": "remove_export", "name": "unusedFn", "line": 42 },
    { "path": "package.json", "action": "remove_dependency", "name": "lodash" }
  ],
  "total_changes": 2
}
```

## Combined output (`fallow` with no subcommand)

When running `fallow` with no subcommand (all analyses), the JSON output combines results from all enabled analyses:

```json
{
  "kind": "combined",
  "schema_version": 7,
  "version": "3.49.0",
  "elapsed_ms": 159,
  "check": {
    "schema_version": 7,
    "version": "3.49.0",
    "elapsed_ms": 45,
    "total_issues": 12,
    "unused_files": [],
    "unused_exports": [],
    "unused_types": [],
    "unused_dependencies": [],
    "unused_dev_dependencies": [],
    "unused_enum_members": [],
    "unused_class_members": [],
    "unresolved_imports": [],
    "unlisted_dependencies": [],
    "duplicate_exports": [],
    "circular_dependencies": [],
    "re_export_cycles": [],
    "boundary_violations": [],
    "unused_optional_dependencies": [],
    "type_only_dependencies": [],
    "test_only_dependencies": [],
    "stale_suppressions": []
  },
  "dupes": {
    "total_clones": 15,
    "total_lines_duplicated": 230,
    "duplication_percentage": 4.2,
    "clone_groups": []
  },
  "health": {
    "summary": {},
    "findings": [],
    "vital_signs": {}
  }
}
```

Use `--only` or `--skip` to control which analyses are included in the combined output. The `architecture` value selects the import cycles, boundary violations and policy violations of the dead-code analysis: `--only architecture` reports only them, and `--skip architecture` removes them. Human output shows them in an "Architecture" category after the other dead-code categories, with and without `--quiet`, and also with `--group-by` and `--summary`. Use `--coverage` and `--coverage-root` to feed Istanbul coverage data to the embedded health analysis for exact CRAP scoring.

With `--score`, the combined output's `health` section includes a `health_score` object (same schema as `health --score`). With `--trend`, it includes a `health_trend` object comparing against the most recent saved snapshot. With `--save-snapshot`, a vital signs snapshot is persisted for future trend comparisons.

## Error output (exit code 2)

```json
{"error": true, "message": "invalid config: unknown field 'detect'", "exit_code": 2}
```
