# `fix`: Auto-Remove Unused Code

Auto-removes unused exports, dependencies, enum members, and pnpm catalog entries.

## Flags

<!-- generated:flags:fix:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--dry-run` | `bool` | `false` | Show what would be removed without modifying files. For `add-to-config` actions, prints a unified-diff preview of the proposed config write; JSON mode includes the diff under a `proposed_diff` field on the fix entry. |
| `--yes` | `bool` | `false` | Skip confirmation prompt (**required** in non-TTY) |
| `--no-create-config` | `bool` | `false` | Refuse to create a new `.fallowrc.json` when none exists. The duplicate-export config-add path is skipped with `skip_reason: "no_create_config"`; source-file edits proceed normally. Use in pre-commit hooks, CI bots, and `fallow watch` where silently materialising a new top-level file would surprise the user. |

Common global flags for this command: [`--format`](global-flags.md), [`--quiet`](global-flags.md).
<!-- generated:flags:fix:end -->

`--force` is an alias for `--yes`.

## What gets fixed

- Unused exports (removes the `export` keyword; whole-enum block when every member is unused). In an `export { ... }` list, on one line or over several lines, it removes the unused names and keeps the layout and comments of the names that stay. It deletes the full statement when no name is left.
- Unused dependencies (removed from `package.json`)
- Unused enum members (removed from the declaration). A kept member that counts on from a removed member gets an initializer with its old value, as in `Medium = 1`, so no kept value changes. When that old value is not a known integer (a computed initializer, or an ambient enum: a `declare enum`, an enum in a `declare` block, or an enum in a `.d.ts` file), the whole enum stays as it is. Each unused member of that enum stays too, and the fix output has no entry for it.
- Unused pnpm catalog entries (removed from `pnpm-workspace.yaml` by line-aware deletion). Object-form entries are removed as one block. By default, fallow also removes a contiguous YAML comment block immediately above the entry when it clearly belongs to that entry; configure this with `fix.catalog.deletePrecedingComments` (`"auto"`, `"always"`, or `"never"`). Two escape hatches keep curated comments safe regardless of policy: a `# fallow-keep` marker on any line in the block preserves it, and the `auto` policy additionally preserves section-banner blocks whose body starts with three or more `=`, `-`, `*`, `_`, `~`, `+`, or `#` characters (e.g. `# === React 18 production pins ===`). Other comments and stylistic choices are preserved. When the last entry of a catalog group is removed, the header is rewritten to `catalog: {}` / `<name>: {}` so pnpm doesn't reject the resulting null value. Entries with non-empty `hardcoded_consumers` are skipped to avoid breaking `pnpm install`; the skip is surfaced in the JSON fix output as `{"type": "remove_catalog_entry", "applied": false, "skipped": true, "skip_reason": "hardcoded_consumers", "consumers": [...]}`. The JSON action carries both `line` (first deleted line, the leading comment when policy absorbs one) and `entry_line` (the catalog entry's original 1-based line); use `entry_line` as a stable anchor across policy changes. After a successful catalog edit the CLI emits a one-line `Run pnpm install to refresh pnpm-lock.yaml` reminder, and the human stderr summary appends `(+M catalog comment lines)` to the fixed-issue count when comment lines were absorbed. The JSON envelope carries a top-level `"skipped"` count alongside `"total_fixed"` for partial-fix gating.
- Duplicate exports (appends an `ignoreExports` rule to your fallow config file). When no fallow config file exists, `.fallowrc.json` is created using the same scaffolding `fallow init` would emit (framework detection, `$schema`, `entry`, `ignorePatterns`, etc.) and the rules are layered on top. Inside a monorepo subpackage (`pnpm-workspace.yaml`, `package.json#workspaces`, `turbo.json`, `lerna.json`, or `rush.json` above the invocation directory) the create-fallback refuses to fire and emits `skip_reason: "monorepo_subpackage"` with a relative `workspace_root` path pointing at the workspace root. When the fallow config file is a symbolic link that resolves outside the project scope base (the git top level of the root), the config-add path does not write through it and emits `skip_reason: "config_link_outside_scope"`. The applied entry carries `created_files: [".fallowrc.json"]` so consumers can detect file-creation side effects programmatically.

## On-disk drift protection

`fallow fix` captures every parsed source file's xxh3 content hash during the in-process analysis and recomputes it at fix time. Files whose hash drifted between analysis and write (parallel editor save, CI rebase, concurrent tool) are skipped with `{"type": "skipped", "path": "...", "skipped": true, "skip_reason": "content_changed"}` in the JSON output and `Skipping <path>: file content changed since fallow dead-code ran. Re-run fallow fix to refresh the analysis first.` on stderr (gated on non-quiet). A run with any content-changed skip exits with code 2 so CI does not treat the partial run as a clean no-op. The JSON envelope's top-level `skipped_content_changed: number` is always present and disjoint from `skipped` (which still tallies catalog / YAML guard skips only). Per-file writes are batched: each rewrite is staged to a sibling temp file, and the orchestrator promotes the batch only after every stage succeeds. A stage failure leaves every target file at its original content. Hash precondition covers source files (TS, JS, Vue, Svelte, Astro, MDX); `package.json` and `pnpm-workspace.yaml` are not in the captured hash map because the extract layer does not parse them, but the dep and catalog fixers re-parse those files at fix time as the natural safety net.

## Low-confidence removals

Issue #602: `fallow fix` withholds removals when the consumer may be invisible to static analysis, because stripping a real export breaks `tsc` and the build. Three cases are skipped:

- **Off-graph consumer directories.** The file is under any of `__mocks__`, `__fixtures__`, `fixtures`, `e2e`, `e2e-tests`, `cypress`, `playwright`, `examples`, `evals`, `golden` (matched on any path segment). Catches Vitest mock aliases, off-workspace e2e suites, and fixture / golden harnesses. Plain `test` / `tests` / `__tests__` are deliberately NOT on the list, so genuinely-dead test helpers still auto-remove.
- **Files with an unresolved import.** The file itself imports something fallow could not resolve, so its local usage graph is incomplete.
- **Findings a file the run never fully analyzed could have distorted.** The dead-code finding carries `reachability_caveats`, meaning some source file was skipped before it was read, could not be read, or did not parse cleanly, so the import that would have credited the export or the package may never have been seen. A file over the per-file size limit is the case that fires at default settings: its first line can import the very module now reported unused. This case also covers `remove-dependency`, the most destructive write `fallow fix` performs: a package is reported unused only when no module imports its specifier, and an unread file hides exactly that import. Resolve the files named in `workspace_diagnostics[]`, then re-run.

JSON output carries `{"type": "skipped", "path": "...", "skipped": true, "skip_reason": "low_confidence_off_graph"}` (or `"low_confidence_unresolved_imports"`, or `"low_confidence_incomplete_analysis"`) plus a top-level counter `skipped_low_confidence_exports: number` (always present), disjoint from `skipped`. A withheld `remove_dependency` entry keeps its own shape (`type`, `package`, `location`, `file`) with `applied: false`, `skipped: true`, the same `skip_reason`, and is counted by the sibling `skipped_low_confidence_dependencies: number`. Those entries also repeat the finding's `reachability_caveats` token array, so gate on that rather than on the reason string. For `low_confidence_incomplete_analysis`, the top-level `incomplete_analysis_diagnostics` array names each path that the run did not fully analyze and the `action` that unblocks the fix (for example `!.claude/**` in `ignorePatterns`); it is omitted when no fix is held back for this reason. Unlike the drift and encoding skips this is INTENTIONAL and does NOT change the exit code; the finding stays reported by `fallow dead-code` for manual review. High-confidence exports in normal source files are removed unchanged. The AI agent should report kept exports and packages to the user and let them decide whether the finding is truly unused before removing it by hand.

## Exports that a kept re-export names

`fallow fix` never removes the `export` of a declaration while a re-export that the run keeps still names it, because the type check of the fixed code then fails (TS2459). A re-export stays when `fix` does not remove it: for example, the file is skipped as low confidence, `ignoreExports` hides its names, the positional path scope excludes it, or the list statement has no safe text edit. Each withheld removal is a `remove_export` entry with `applied: false`, `skipped: true` and `skip_reason: "kept_re_export"`, counted by the top-level `skipped_kept_re_exports: number` (always present), disjoint from `skipped`. Remove the re-export first, then run `fallow fix` again. This is intentional and does not change the exit code.

Limit: the check reads the re-exports from the module graph. A file that `ignorePatterns` excludes is not in the graph, so a re-export in such a file cannot hold a removal back. When an ignored file re-exports project code, run the type check after `fallow fix`.

## Declarations that export more than one name

One `export` keyword can export several names: `export const { a, b } = x;`, `export const [a, b] = x;` or `export const a = 1, b = 2;`. `fallow fix` removes that keyword only when every name of the declaration is unused. While one name is in use or kept, each unused name of the declaration is a `remove_export` entry with `applied: false`, `skipped: true` and `skip_reason: "shared_declaration_in_use"`, counted by the top-level `skipped_shared_declarations: number` (always present), disjoint from `skipped`. `fix` does not split the declaration, because a split evaluates the initializer twice and changes the value of a rest element. Move each unused name to a declaration without `export` by hand. This is intentional and does not change the exit code.

## Post-edit check

Before `fallow fix` writes a file, it applies the planned edits to the text and checks the result. It keeps an edit only when:

- the edited file still parses;
- each kept class member and enum member stays, in the same order;
- each kept enum member keeps its constant value;
- each name that the code uses still has a declaration or an import;
- no import stays whose name only a removed export list used;
- declarations that TypeScript merges (function overloads, a class with its interface or namespace, a function with its namespace) stay all exported or all local;
- each removed export is an unused export, and no kept re-export reads it.

`fix` keeps the code of an edit that fails the check, and the JSON output lists no entry for it. The human output prints `Kept <path>:<line> as it is: <reason>` on stderr, and no `Would remove` or `Removed` line for that edit. The finding stays in the next report. The editor quick fix and the `suggestion` blocks of the GitHub and GitLab review comments use the same check.

## Generated files

`fallow fix` does not remove an unused export, enum member or class member in a generated file, because the generator writes the file again on its next run. A file is generated when its path has a `generated` or `__generated__` folder or a `.generated.` or `.gen.` name part, or when its leading comment has `@generated`, or names the generator together with "do not edit" (`// Generated by <tool>. Do not edit.`). The file is a `{"type": "skipped", "path": "...", "skipped": true, "skip_reason": "generated_source"}` entry with a `help` text. This is intentional and does not change the exit code, and the entry is not counted in `skipped`. Change the input of the generator, or add the file to `ignoreExports`. In the JSON of `fallow dead-code`, an unused export or unused type in such a file has `auto_fixable: false` on its `remove-export` action, no `suppress-line` action, and an `add-to-config` action with the `ignoreExports` rule for the file. An unused export in a shadcn/ui folder (the `ui` alias of a `components.json` with the shadcn/ui schema) has the same kind of `add-to-config` action, with a rule for the folder. The human report prints one hint with the config entry for each group of 3 findings or more.

## Exports that test-side files import

`fallow fix --production` does not remove an unused export that a reachable test, story, fixture or bench file, a `production.ignore` file, or a file that only a support entry seeds imports, because the removal breaks that import. The export is a `remove_export` entry with `applied: false`, `skipped: true`, `skip_reason: "test_importers"` and `help`, counted by the top-level `skipped_test_importers: number` (always present), disjoint from `skipped`. This is intentional and does not change the exit code. In the JSON of `fallow dead-code --production`, such an `unused_exports` or `unused_types` finding keeps `remove-export` as its first action, with `auto_fixable: false` and the reason in `note`, and has a `test_importers` object: `files` (up to 5 root-relative paths) and `remaining` (the number of other importing files). Change or remove the import first, or keep the export.

## File encoding contract

`fallow fix` is UTF-8 only. Two encoding shapes that previously caused silent corruption are handled explicitly (issue #475):

- **UTF-8 BOM round-trip.** Files with a leading UTF-8 byte-order mark (`EF BB BF`, common on Windows-authored TypeScript) are read with the BOM stripped before line-offset computation and parsing, so reported line numbers do not shift by the BOM codepoint, and the BOM is re-prepended on write so the file's encoding is preserved on round-trip. fallow neither adds nor removes a BOM; if your input has one, the output has one.

- **Mixed CRLF / LF rejection.** Files containing both `\r\n` and bare-LF line endings (common after cross-platform edits without `core.autocrlf`) are skipped instead of silently rewritten to the wrong offsets. The stderr message names the remediation: `Skipping <path>: file has mixed CRLF/LF line endings. Normalize with dos2unix or set git config core.autocrlf input, then re-run fallow fix.`. JSON output carries `{"type": "skipped", "path": "...", "skipped": true, "skip_reason": "mixed_line_endings"}` plus a top-level counter `skipped_mixed_line_endings: number` (always present) disjoint from `skipped_content_changed`. Any non-zero mixed-EOL count exits the run with code 2.

  **The skip is NOT self-healing**. Re-running `fallow fix` produces the same skip; the AI agent or user must run `dos2unix <path>` (or set `git config core.autocrlf input` and re-checkout) before fallow can act on the file. When the same file carries findings for multiple fixers (e.g. an unused export AND an unused enum member), the skip is reported once per file, not once per fixer.

## Examples

```bash
# Preview changes
fallow fix --dry-run --format json --quiet

# Apply changes (--yes required in agent/CI environments)
fallow fix --yes --format json --quiet
```
