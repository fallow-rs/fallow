# Conformance Test Suite

This suite compares fallow's dead code detection with [knip](https://github.com/webpro-nl/knip) on real-world open-source projects. It reports agreements and disagreements.

This suite is informational. It does not fail on disagreements. Differences between the tools are expected due to different analysis strategies and heuristics.

## Test Projects

The same projects used by the performance benchmark suite:

| Project | Repo | Size |
|---------|------|------|
| preact | preactjs/preact | Small |
| fastify | fastify/fastify | Small |
| zod | colinhacks/zod | Small |
| vue-core | vuejs/core | Large monorepo |
| svelte | sveltejs/svelte | Large monorepo |
| query | TanStack/query | Large monorepo |
| vite | vitejs/vite | Large monorepo |
| next.js | vercel/next.js | XL monorepo |

## Prerequisites

- **fallow** binary (built from this repo or in PATH)
- **Node.js** (v22+) with `npx` available
- **pnpm** (for monorepo projects)
- **Python 3** (for the comparison and aggregation logic)
- knip is installed automatically via `npx` on first run

## Running

### All projects (CI mode)

```bash
cargo build --release
./tests/conformance/run-all.sh --fallow-bin ./target/release/fallow

# With custom clone directory and timeout
./tests/conformance/run-all.sh \
  --fallow-bin ./target/release/fallow \
  --clone-dir /tmp/fallow-conformance \
  --timeout 300
```

### Single project

```bash
# Against a specific project
./tests/conformance/run.sh /path/to/your/project

# With a specific fallow binary
./tests/conformance/run.sh --fallow-bin ./target/debug/fallow
```

Both scripts output:
- **stderr**: human-readable summary with issue counts and disagreement details
- **stdout**: structured JSON report (pipe to `jq` or save to file)

```bash
# Save JSON report
./tests/conformance/run-all.sh > report.json

# Pretty-print with jq
./tests/conformance/run-all.sh 2>/dev/null | jq .
```

## Interpreting Results

The report breaks down findings into three categories:

- **Agreed**: Issues found by both fallow and knip. Agreement does not independently establish that these are real issues.
- **Fallow-only**: Issues found by fallow but not knip. Could be:
  - True positives that knip misses (fallow wins)
  - False positives in fallow (needs investigation)
- **Knip-only**: Issues found by knip but not fallow. Could be:
  - True positives that fallow misses (needs implementation)
  - False positives in knip (fallow correctly ignores them)

The agreement percentage is calculated as `agreed / total_unique_issues * 100`.

## Adjudication Register

The agreement percentage does not establish which tool is correct. The report
joins every disagreement row against `adjudications.json`, keyed on
`(project, issue_type, path, export_name)`. Each record has a `verdict`
(`fallow_wrong`, `knip_wrong`, or `model_difference`), a `cause` from the
pipeline vocabulary the `debug-false-positive` skill uses, plus the
`knip_version`, `corpus_ref`, `reviewed_on` date, and the tracking `issue`.

The aggregated summary reports `adjudicated_fallow_wrong`,
`adjudicated_knip_wrong`, `adjudicated_model_difference`, and `unadjudicated`.
The four buckets partition the disagreements. `unadjudicated` records the
backlog that has not been reviewed. An unknown verdict, an unknown cause, a duplicate
key, or a record missing a declared field is a hard error.

Write the verdict before the fix. The `conformance-loop` skill makes this a
separate step so classifications go into the register instead of remaining
only in session context or the ignored `.plans/` directory.

The register does not gate CI. The lane stays `continue-on-error: true` while
the unadjudicated count settles.

## Issue Type Mapping

| Fallow                    | Knip            |
|---------------------------|-----------------|
| `unused_files`            | `files`         |
| `unused_exports`          | `exports`       |
| `unused_types`            | `types`         |
| `unused_dependencies`     | `dependencies`  |
| `unused_dev_dependencies` | `devDependencies`|
| `unresolved_imports`      | `unresolved`    |
| `unlisted_dependencies`   | `unlisted`      |
| `duplicate_exports`       | `duplicates`    |
| `unused_enum_members`     | `enumMembers`   |
| `unused_class_members`    | `classMembers`  |

## Scripts

| Script | Purpose |
|--------|---------|
| `run.sh` | Single-project comparison (fallow vs knip) |
| `run-all.sh` | Multi-project orchestrator (clone, run, aggregate) |
| `compare.py` | Normalizes and compares tool outputs for one project |
| `aggregate.py` | Combines per-project reports into overall summary, joined against the adjudication register |
| `adjudications.json` | Reviewed verdicts and causes for individual disagreements |

## Aggregated JSON Report Schema

```json
{
  "summary": {
    "fallow_total": 150,
    "knip_total": 120,
    "agreed": 100,
    "fallow_only": 50,
    "knip_only": 20,
    "agreement_pct": 58.8,
    "adjudicated_fallow_wrong": 4,
    "adjudicated_knip_wrong": 6,
    "adjudicated_model_difference": 10,
    "unadjudicated": 50
  },
  "projects": {
    "preact": { "fallow_total": 10, "knip_total": 8, "agreed": 6, ... },
    "vite": { "fallow_total": 20, "knip_total": 15, "agreed": 12, ... }
  },
  "by_type": {
    "unused_exports": {
      "fallow_count": 50,
      "knip_count": 40,
      "agreed": 35,
      "fallow_only": 15,
      "knip_only": 5,
      "agreement_pct": 63.6
    }
  }
}
```
