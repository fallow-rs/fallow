# `dupes`: Duplication Detection

Finds code duplication and clones across the project.

By default, `fallow dupes` skips generated framework and tool output matching `**/.next/**`, `**/.nuxt/**`, `**/.svelte-kit/**`, `**/.turbo/**`, `**/.parcel-cache/**`, `**/.vite/**`, `**/.cache/**`, `**/out/**`, and `**/storybook-static/**`, and test and mock files matching `**/*.test.*`, `**/*.spec.*`, `**/__tests__/**`, and `**/__mocks__/**`. These defaults merge with `duplicates.ignore`. Set `duplicates.ignoreDefaults = false` to opt out and use only your configured ignore list. If the reported duplication percentage drops after upgrading, these default ignores are the expected reason.

## Flags

<!-- generated:flags:dupes:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--mode` | `strict\|mild\|weak\|semantic` | - | Detection mode |
| `--near` | `bool` | `false` | Enable function-scoped near-miss clone detection |
| `--min-tokens` | `string` | - | Minimum token count for a clone |
| `--min-lines` | `string` | - | Minimum line count for a clone |
| `--min-occurrences` | `string` | - | Minimum number of occurrences before a clone group is reported (must be ≥ 2). Raise to skip pair-only clones and focus on widespread copy-paste worth refactoring. `fallow init` writes `minOccurrences: 3` into new projects. |
| `--skip-local` | `bool` | `false` | Only report cross-directory duplicates |
| `--cross-language` | `bool` | `false` | Strip type annotations for TS↔JS matching |
| `--ignore-imports` | `bool` | `false` | Exclude module wiring from clone detection |
| `--no-ignore-imports` | `bool` | `false` | Count module wiring as clone candidates (opt out of the default exclusion) |
| `--ignore-symlinks` | `bool` | `false` | Omit clone instances whose path is a symlink, or lies under a symlinked directory. A clone group with fewer than two remaining instances is not reported. Without this flag, JSON output marks these instances with `is_symlink: true` |
| `--no-ignore-symlinks` | `bool` | `false` | Report symlinked clone instances (opt out of a config `duplicates.ignoreSymlinks: true`) |
| `--top` | `string` | - | Show only the N highest-ranked clone groups. Ranking multiplies token count and occurrences, then adds a capped spread boost for distant files or same-file locations. `clone_families[]` narrows with the groups. Summary stats reflect the scoped project; `clone_groups_shown` / `clone_groups_omitted` and `clone_families_shown` / `clone_families_omitted` report both splits. Refused with exit code 2 alongside `--group-by`, which reports per-bucket stats over every clone group in a bucket that a global top-N truncation would contradict. |
| `--no-fragments` | `bool` | `false` | Omit the verbatim source text from each clone instance in `--format json`. The file and line/column range still address the same code, and this is most of the payload on a duplicated codebase |
| `--trace` | `string` | - | Deep-dive clones. `FILE:LINE` traces all clones at a location; `dup:<id>` traces a clone group by the stable fingerprint shown in the listing and on `clone_groups[].fingerprint` in JSON. Fingerprints are usually `dup:<8hex>` and widen only on rare report collisions. Trace output adds an extract-function suggestion, estimated savings, and a best-effort proposed name per group |
| `--threshold` | `string` | - | Fail if duplication exceeds this percentage |

Common global flags for this command: [`--format`](global-flags.md), [`--quiet`](global-flags.md), [`--changed-since`](global-flags.md), [`--baseline`](global-flags.md), [`--save-baseline`](global-flags.md), [`--workspace`](global-flags.md), [`--changed-workspaces`](global-flags.md), [`--group-by`](global-flags.md), [`--explain-skipped`](global-flags.md).
<!-- generated:flags:dupes:end -->

With `--group-by`, each clone group goes to its **largest owner** (the most instances; an alphabetical tiebreak): a group split 2 `src` / 1 `lib` appears under `src`. JSON adds `grouped_by` plus a `groups` array. Each bucket carries dedup-aware `stats`, `clone_groups` (each group has `primary_owner` and a per-instance `owner`), and `clone_families`. SARIF results carry `properties.group`, and CodeClimate issues carry a top-level `group` field. Compact and markdown output fall back to ungrouped output with a stderr note.

## Detection Modes

| Mode | Behavior |
|------|----------|
| `strict` | Exact token match (no normalization) |
| `mild` | Syntax normalized (whitespace, semicolons) |
| `weak` | Different literal values treated as equivalent |
| `semantic` | Renamed variables also treated as equivalent |

Near-miss detection is an independent opt-in, not another normalization mode.
Near groups include `similarity`; every clone-group finding includes `spread`.

## Examples

```bash
# Default duplication scan
fallow dupes --format json --quiet

# Semantic mode (detects renames)
fallow dupes --format json --quiet --mode semantic

# Cross-directory only, fail at 5%
fallow dupes --format json --quiet --skip-local --threshold 5

# Trace clones at a specific location
fallow dupes --format json --quiet --trace src/utils.ts:42

# Deep-dive a clone group by its dup:<id> fingerprint (from the listing or JSON)
fallow dupes --format json --quiet --trace dup:7f3a2c1e

# Only check duplication in changed files
fallow dupes --format json --quiet --changed-since main

# Incremental CI
fallow dupes --format json --quiet --save-baseline fallow-baselines/dupes.json
fallow dupes --format json --quiet --baseline fallow-baselines/dupes.json --threshold 5
```
