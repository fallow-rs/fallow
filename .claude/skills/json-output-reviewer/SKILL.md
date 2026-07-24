---
name: json-output-reviewer
description: Reviews JSON output schema design, backwards compatibility, actions arrays, and machine-readability. Use when reviewing changes to fallow's JSON output format.
---
<!-- Generated from .agents/skills. Do not edit. -->

Review changes to fallow's JSON output format. This is the primary machine interface consumed by agents, CI pipelines, and integrations.

## What to check

1. **Schema stability**: Breaking changes to existing fields require a `schema_version` bump. Never rename, remove, or change the type of an existing field without versioning
2. **Actions arrays**: Every issue must include an `actions` array with machine-actionable fix and suppress hints. Check `auto_fixable` is set correctly
3. **Consistent naming**: snake_case for all field names, no abbreviations, no inconsistency between commands (e.g., `unused_exports` not `unusedExports`)
4. **Null vs absent**: Absent means "not computed" (flag not set), `null` means "computed but no value". Never mix these semantics
5. **Metadata with `--explain`**: `_meta` objects must include value ranges, definitions, and interpretation hints for every numeric field
6. **Grouped output**: When `--group-by` is active, the envelope changes to `{ grouped_by, total_issues, groups: [...] }`. Verify both grouped and ungrouped paths
7. **Error output**: Exit code 2 errors must emit `{"error": true, "message": "...", "exit_code": 2}` on stdout, not stderr
8. **Determinism**: Same input must produce byte-identical JSON output. No random ordering, no timestamps unless explicitly requested

## Surface-specific checks

For each JSON-output diff, walk this list in addition to the generic checks above:

- [ ] **Closed-enum field violations across hand-rolled emit paths**: when a new code path constructs a struct with `Serialize`-derived `String` fields that the published schema constrains to closed enums (`docs/output-schema.json` `enum: [...]`), grep every literal string emitted into those fields and confirm membership. Concrete recipe: for each new `<StructWithSchema> { field: "...".to_owned(), }` in the diff, run `grep -nE '"<field>"' docs/output-schema.json | head -5` to find the schema definition, read the `enum` constraint, and confirm every emitted literal is in the list. Heuristic for which fields are at risk: the local sidecar / canonical path emits one set of values, the new hand-rolled path emits another; if the rust struct field type is `String` (not a `#[serde(rename_all = "snake_case")] enum`), the compiler will not catch drift. Pattern target list for runtime coverage: `evidence.static_status` (`["used","unused"]`), `evidence.test_coverage` (`["covered","not_covered"]`), `evidence.v8_tracking` (`["tracked","untracked"]`), `verdict` (already enum-typed, safe). Caught 2026-04-30 on `fallow coverage analyze --cloud`: hand-rolled `merge_cloud_snapshot` emitted `test_coverage: "unknown"` and `v8_tracking: "never_called"` outside the schema enums; compile + clippy + 94 unit tests + 2 integration tests all passed.
- [ ] **Plugin-count drift sweep across non-companion surfaces**: when the change bumps the plugin count, beyond the companion-repo flag-gate sweep AND the existing `README.md` / detection.md updates, ALSO grep these additional in-repo and internal locations for stale counts:
  ```bash
  /usr/bin/grep -nE "\b(89|90|91|92|93)\b.*plugin" \
    .claude/rules/plugins.md \
    .claude/rules/core-crate.md \
    npm/fallow/package.json \
    npm/fallow/README.md
  ```
  Each hit must either use the new count or have an explicit reason to lag, such as a historical table. Treat `npm/fallow/skills/**` as a vendored release artifact rather than a canonical edit target. The registry defines what to compare, while this search identifies every current consumer that must move with it.

### JSON format audit (Phase 3a)

The real-world corpus is intentionally untracked. Before its first use, follow
the [benchmark setup](../../../BENCHMARKS.md#comparative-benchmarks) and run
`npm --prefix benchmarks run download-fixtures`.

```bash
FALLOW_QUIET=1 fallow <command> --format json --root benchmarks/fixtures/real-world/zod 2>/dev/null | jq . | head -c 2000
```

Check:
- [ ] All paths are relative (no absolute paths leaking)
- [ ] `schema_version` present and correct
- [ ] New fields use correct types (int vs float vs string)
- [ ] Optional fields use `skip_serializing_if`, omitted when not applicable
- [ ] When feature flag is OFF, new fields are completely absent from JSON (not null, not empty)

## Key files

- `crates/cli/src/report/json.rs` (main JSON serialization)
- `crates/cli/src/report/mod.rs` (format dispatch, schema_version constant)
- `crates/types/src/results.rs` (result types that become JSON)

## Veto rights

Can **BLOCK** on:
- Breaking schema changes without `schema_version` bump
- Missing `actions` arrays on issues
- Non-deterministic output (random field ordering)
- Error output on stderr instead of structured JSON on stdout

## Output format

End with a verdict:

```
## Verdict: APPROVE | CONCERN | BLOCK
```

## What NOT to flag

- Human output formatting
- Internal struct layout (only the serialized output matters)
- Performance of serialization (serde is fast enough)
