# `audit`: Changed-File Quality Gate

Audits changed files for dead code, complexity, duplication, and styling. Returns a verdict (pass/warn/fail). Purpose-built for PR quality gates and reviewing AI-generated code. When `--base` is not set, the base is the `git merge-base` against the branch's upstream or the remote default (`origin/HEAD`, `origin/main`, `origin/master`); set `FALLOW_AUDIT_BASE` to pin it without a flag. Defaults to `--gate new-only`, which fails only on findings introduced by the current changeset and reports inherited findings as context. Under `new-only`, the audit also reports an unused file or unused export outside the changed files when the base does not report it. Such a finding is introduced: the change removed the last use of that code, or a config change (for example a removed entry point) made it unreachable. An unused file or unused export outside the changed files that the base also reports stays out of the report.

## Flags

<!-- generated:flags:audit:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--production-dead-code` | `bool` | `false` | Per-analysis production mode for the dead-code sub-analysis only |
| `--production-health` | `bool` | `false` | Per-analysis production mode for the health sub-analysis only |
| `--production-dupes` | `bool` | `false` | Per-analysis production mode for the duplication sub-analysis only |
| `--dead-code-baseline` | `string` | - | Baseline file (produced by `fallow dead-code --save-baseline`). Pre-existing dead-code issues are excluded from the verdict. |
| `--health-baseline` | `string` | - | Baseline file (produced by `fallow health --save-baseline`). Pre-existing complexity findings are excluded from the verdict. |
| `--dupes-baseline` | `string` | - | Baseline file (produced by `fallow dupes --save-baseline`). Pre-existing clone groups are excluded from the verdict. |
| `--max-crap` | `string` | - | Forwarded to the health sub-analysis. Functions meeting or exceeding this CRAP score cause audit to fail. Same formula as `health --max-crap`. Pair with coverage data for accurate per-function CRAP. |
| `--coverage` | `string` | - | Path to Istanbul coverage data (`coverage-final.json`) or raw V8 coverage (a `NODE_V8_COVERAGE` directory or one V8 JSON file) for accurate per-function CRAP scores in the health sub-analysis. Same formats and semantics as `health --coverage`. Also configurable via `FALLOW_COVERAGE`, then `health.coverage` (the same chain as `fallow health`). Relative paths resolve against `--root`. |
| `--coverage-root` | `string` | - | Absolute prefix to strip from file paths in coverage data before prepending the project root. Also configurable via `FALLOW_COVERAGE_ROOT`, then `health.coverageRoot`. Use when coverage was generated under a different checkout root in CI / Docker (e.g., `/home/runner/work/myapp` on GitHub Actions). |
| `--only` | `dead-code\|dupes\|health\|architecture` | - | Run only these sub-analyses (comma-separated: dead-code, architecture, dupes, health), with the same values as bare `fallow --only`. Replaces the `audit.deadCode`, `audit.health` and `audit.dupes` selection. The review brief (`--brief`, `fallow review`) keeps every sub-analysis |
| `--skip` | `dead-code\|dupes\|health\|architecture` | - | Skip these sub-analyses (comma-separated: dead-code, architecture, dupes, health), with the same values as bare `fallow --skip`, on top of the `audit.*` selection |
| `--no-css` | `bool` | `false` | Disable styling analytics in audit |
| `--css-deep` | `bool` | `false` | Enable deep CSS analysis for audit explicitly: project-wide styling reachability, narrowed back to changed anchors. Deep CSS is on by default; use this to override `audit.cssDeep = false` |
| `--no-css-deep` | `bool` | `false` | Disable deep CSS analysis while keeping local styling analytics on |
| `--gate` | `new-only\|all` | - | Which findings affect the verdict. `new-only` gates only introduced findings, including unused files and exports outside the changed files that the base does not report; `all` gates every finding in changed files and skips the extra base-snapshot attribution pass. |
| `--worsened` | `warn\|error\|off` | - | How a worsened complexity finding affects the verdict under `new-only`. A worsened finding was already in the base, and a measured value is higher now. The line count must grow by at least 5 lines, and it includes comments and blank lines. `warn` (default) gives at least a `warn` verdict. `error` counts the finding like an introduced finding. `off` reports no worsened finding. A grown exceeded metric stays introduced in both modes. |
| `--runtime-coverage` | `string` | - | Runtime coverage input. Accepts a V8 directory, a single V8 JSON file, or an Istanbul coverage map JSON. Runs the `fallow-cov` sidecar inside the audit, so the `hot-path-touched` verdict shows next to the dead-code and complexity findings without a second `fallow health` run in CI. The verdict is informational and does not change the exit code. A single local capture is free. Continuous or multi-capture monitoring needs a license (see `fallow license`) |
| `--min-invocations-hot` | `string` | `100` | Threshold for hot-path classification, forwarded to the sidecar when `--runtime-coverage` is set |
| `--mutation-report` | `string` | - | Saved `fallow mutation --format json` report (for example `.fallow/mutation/last-report.json`). The review brief counts the surviving mutants on the added lines of each changed file in the `mutation` component of the focus score. A file whose source changed since the run adds nothing. Reads the file only; it runs no test. Only the review brief reads it |
| `--gate-marker` | `string` | - | Internal marker identifying a gate run (e.g. `pre-commit`), set by the generated git hook so Fallow Impact can record a containment event when the gate blocks then clears. Hidden; never changes the verdict, exit code, or output |
| `--staged` | `bool` | `false` | Audit the staged index instead of the working tree. The changed files are the staged changes against HEAD (`git diff --cached`), and the analysis reads every file from the index, so unstaged edits and untracked files do not affect the verdict. A file that is staged and then deleted from the working tree is still audited. The base is HEAD, so `--base` and `--changed-since` are rejected. The generated pre-commit hook uses this flag |
| `--brief` | `bool` | `false` | Render the deterministic review brief instead of the gating audit report. The brief answers "where do I look?" rather than "will CI block this?", runs the same analysis, and ALWAYS exits 0 (the verdict is carried informationally). Implied by `fallow review`. Orthogonal to `--format` |
| `--max-decisions` | `string` | `4` | Cap on the number of consequential structural decisions surfaced in the review brief's decision surface (the working-memory limit). Default 4; clamped to the 3-5 band (4 plus or minus 1). Only consulted on the brief path |
| `--walkthrough-guide` | `bool` | `false` | Emit the agent-contract WALKTHROUGH GUIDE: the current digest (brief + decision surface), the review direction, the JSON schema the agent must return, and a deterministic graph-snapshot hash pinned into the digest. The digest is built from the graph only (PR prose is never folded in, so it is injection-resistant). Implies the brief; always exits 0. A thin agent skill calls this to fetch the current guide, produces judgment JSON, then reopens with `--walkthrough-file` |
| `--walkthrough-file` | `string` | - | Ingest an agent's judgment JSON and POST-VALIDATE it against the LIVE graph. Rejects any judgment whose `signal_id` fallow did not emit (anti-hallucination); refuses the whole payload as stale when the echoed graph-snapshot hash no longer matches (the tree moved). The verifier is the graph, not a second model. Implies the brief; always exits 0. The agent's free-text framing is fenced as non-deterministic and never gates or auto-posts |
| `--walkthrough` | `bool` | `false` | Render the existing walkthrough guide as a staged HUMAN terminal tour (Stage 1 load-bearing / Stage 2 mechanical), or markdown with `--format markdown`. Implies the brief; always exits 0. `--format json --walkthrough` emits the same agent-contract JSON as `--walkthrough-guide` |
| `--mark-viewed` | `string` | - | Record one or more changed files as VIEWED in the local walkthrough viewed-state ledger (`.fallow/walkthrough-state.json`), then render the tour. Files already viewed (and still current) collapse into the Cleared panel. Repeatable. Stale marks (the tree moved) are ignored on render but never deleted. Only consulted on the `--walkthrough` path |
| `--show-cleared` | `bool` | `false` | Expand the Cleared panel in the human/markdown walkthrough tour: list each de-prioritized and already-viewed file instead of the collapsed one-line summary. Only consulted on the `--walkthrough` path |
| `--show-deprioritized` | `bool` | `false` | Expand the de-prioritized units in the review brief's weighted focus map ("show me what you de-prioritized"). The `deprioritized` escape-hatch list is ALWAYS present in `--format json` regardless; this flag only re-expands the collapse-by-default human focus render. Only consulted on the brief path |

Common global flags for this command: [`--format`](global-flags.md), [`--quiet`](global-flags.md), [`--changed-since`](global-flags.md), [`--diff-file`](global-flags.md), [`--diff-stdin`](global-flags.md), [`--workspace`](global-flags.md), [`--changed-workspaces`](global-flags.md), [`--group-by`](global-flags.md), [`--output-file`](global-flags.md).
<!-- generated:flags:audit:end -->
## Verdicts

| Verdict | Exit code | When |
|---------|-----------|------|
| pass | 0 | No issues in changed files |
| warn | 0 | Issues found, all warn-severity |
| fail | 1 | Error-severity issues found |
| error | 2 | Runtime error (invalid ref, not a git repo) |

With `--gate new-only`, inherited error-severity findings can be present in the JSON output while the verdict remains `pass`; check the `attribution` object and per-finding `introduced` booleans.

A merge of the default branch brings in code that the default branch already has. When the base is auto-detected, audit also compares with the merged default-branch commits: `MERGE_HEAD` of a merge in progress, and the merge-base with the default branch after a merge commit. A finding that one of these commits already has is inherited, so the gate does not block the merge commit or the push after it. Code from a merged branch that the default branch does not contain stays introduced. An explicit `--base` or `FALLOW_AUDIT_BASE` turns this off.

A complexity finding matches its base finding by file path and function name. The line and the exceeded category do not affect the match, and a renamed file keeps its match. A complexity finding is introduced in these cases:

- No base finding matches it.
- The head finding exceeds a metric that the base finding did not exceed, and that metric grew.
- A metric that the head finding exceeds grew, and the finding moved to a higher severity tier (`moderate`, `high`, `critical`).

Growth of a metric that the base finding already exceeded, in the same severity tier, makes the finding `worsened`, not introduced.

An unchanged or decreased metric keeps the finding inherited, also when the exceeded category changes (for example from `both` to `cyclomatic`). When one file has more than one finding with the same function name, each base finding matches at most one head finding. Head findings with unchanged metric values and an unchanged line count match first. Head findings with unchanged metric values match next. Then head findings with unchanged cyclomatic and cognitive values match, also when the CRAP score changed, because a change of the coverage input changes the CRAP score. The other findings then match in line order.

A matched finding that is not introduced is `worsened` when a measured value is higher than in the base finding. The measured values are cyclomatic, cognitive, CRAP (only when both sides have a score) and the line count of the function. The line count must grow by at least 5 lines. It includes comments and blank lines, so the minimum keeps a finding inherited after a short comment or a formatter rewrap. Each complexity finding carries `attribution` (`introduced`, `worsened` or `inherited`). When a value grew, it also carries `worsened_metrics` with the `metric`, the `base` value and the `head` value. An introduced finding that matched a base finding also carries `worsened_metrics`. `attribution.worsened` gives the active mode. `attribution.complexity_worsened` counts the worsened findings, which are also in `complexity_inherited`. `--worsened` (config `audit.worsened`) decides their effect on the verdict:

- `warn` (default): a worsened finding makes the verdict at least `warn`. The exit code stays 0.
- `error`: a worsened finding counts like an introduced finding, so its `complexity-*` rule decides the verdict.
- `off`: growth only in metrics that the finding does not exceed stays inherited. Growth of an exceeded metric stays `worsened` and makes the verdict at least `warn`.

A complexity finding in a test file does not gate the audit verdict, under either gate. A test file is a spec (`*.test.*`, `*.spec.*`, `*.e2e.*`), a file below a test directory (`__tests__/`, `test/`, `tests/`, `e2e/`), a mock or a fixture. Such a finding has `effective_severity: warn` and `gate_exemption: "test-file"`, and it stays in the report with its attribution. It makes the verdict at least `warn`. `fallow health` still reports it at the severity of its rule.

## JSON contract: which fields are severity-aware

| Field | Severity-aware? | What it counts |
|-------|-----------------|----------------|
| `verdict` | **yes** | Overall outcome honoring per-rule severity (`pass` / `warn` / `fail`) |
| `attribution.*_introduced` | no | Findings introduced by the changeset under `gate: new-only`, ignoring severity |
| `summary.*` | no | All findings in changed files, ignoring severity |
| Per-finding `introduced` | no | Whether each finding was introduced by the changeset |

For CI gating and any "did this PR pass?" question, read `verdict` (or exit code). Counting introduced findings ignores severity and breaks projects with `unused-exports: warn`. For agent triage, read `verdict` first, then `attribution` for new-vs-inherited counts, then the per-category finding arrays for actionable details.

The review brief (`fallow audit --brief --format json` and `fallow review --format json`) adds `impact_closure.co_change_gap[]` when a changed file has a hidden change-coupling partner that the diff does not touch. Each entry has `changed_file`, `partner_file`, `shared_revisions`, `degree` and a scope `note`. The pairs use the `health.changeCoupling` thresholds over 12 months of history. The fact is advisory: it is history evidence, not proof that the partner must change, and it never gates. The array is absent when it is empty.

The review brief also adds a top-level `test_reach` object: how tests reach each value export whose declaring statement overlaps an added line. `status` is `measured`, or `no-tests` when the project has no test files. `masking` is `active`, `no-mocks` or `capped` (too many distinct mock sets, so mock detection is off). `counts` has one key per `reach` class: `direct` (a test file references the export), `transitive` (tests reach it only through other modules), `mocked-only` (only tests that mock a module on the import path reference it), `none` (tests load the file but do not reference the export), `none-file` (no test loads the file) and `framework` (a literal framework configuration export, such as the Next.js `dynamic` option). `exports[]` lists at most 25 rows that need attention (`mocked-only`, `none`, `none-file`, `transitive`), with `file`, `export_name`, `line`, `reach` and `change`. `change` is `new` when the file did not export the name at the base of the diff (a new export, a renamed export or an export of a new file), else `modified`. A `mocked-only` row adds `tests`, `tests_total`, `test_changed` and `mocked_targets`. `exports_omitted` counts the rows past the cap. `e2e_runner: true` states that the project uses Playwright or Cypress: those tests drive the running app and do not import files, so the reach classes do not count them. The field is absent when no such runner is present. Treat `reach` as an open set and read an unknown value as "needs attention". The section is advisory and never gates. It is absent when the diff changes no value export.

## Examples

```bash
# Auto-detect base branch
fallow audit --format json --quiet

# Explicit base ref
fallow audit --format json --quiet --base main

# Audit last 3 commits
fallow audit --format json --quiet --base HEAD~3

# Strict mode: fail on inherited findings too
fallow audit --format json --quiet --gate all

# Production code only in a monorepo workspace
fallow audit --format json --quiet --production --workspace @app/api

# Production-only health, full-tree dead-code and dupes
fallow audit --format json --quiet --production-health --workspace @app/api

# CI mode (SARIF + fail on issues + quiet)
fallow audit --ci

# Per-analysis baselines: only fail on genuinely new issues
fallow audit \
  --dead-code-baseline fallow-baselines/dead-code.json \
  --health-baseline    fallow-baselines/health.json \
  --dupes-baseline     fallow-baselines/dupes.json
# Or set these under `audit.*Baseline` in .fallowrc.json so `fallow audit` picks them up with no flags.
# The global --baseline / --save-baseline flags are REJECTED on audit (exit 2) because each sub-analysis uses a different baseline format.
```

## JSON Output Structure

```json
{
  "kind": "audit",
  "schema_version": 7,
  "version": "3.49.0",
  "command": "audit",
  "verdict": "fail",
  "changed_files_count": 12,
  "base_ref": "611d151e8250146426ff3178e94207f8a8d3cc7b",
  "base_description": "merge-base with origin/main",
  "head_sha": "d4a2f91",
  "elapsed_ms": 2140,
  "summary": {
    "dead_code_issues": 2,
    "dead_code_has_errors": true,
    "complexity_findings": 1,
    "max_cyclomatic": 28,
    "duplication_clone_groups": 0
  },
  "attribution": {
    "gate": "new-only",
    "dead_code_introduced": 2,
    "dead_code_inherited": 0,
    "complexity_introduced": 1,
    "complexity_inherited": 0,
    "duplication_introduced": 0,
    "duplication_inherited": 0,
    "styling_introduced": 0,
    "styling_inherited": 0
  },
  "dead_code": {
    "schema_version": 3,
    "total_issues": 2,
    "unused_exports": [{ "path": "src/api.ts", "export_name": "oldApi", "introduced": true, "actions": [...] }]
  },
  "complexity": {
    "findings": [...]
  },
  "duplication": {
    "clone_groups": []
  }
}
```

The `verdict` field is always present and is the primary decision signal. With the default `new-only` gate, the `attribution` object counts introduced vs inherited findings across dead code, complexity, duplication, and styling, and audit sub-results annotate individual findings with `introduced: true/false`. With `gate=all`, audit skips that extra base-snapshot attribution pass, so introduced/inherited counts stay `0` and per-finding `introduced` fields are omitted. A clone group where each instance lies inside a different instance of one base clone group is inherited: an edit inside one copy of a base clone (a changed line, a renamed parameter) splits the base group into smaller groups, and the change did not write that duplication. A pasted copy in a new file has no base lines, so its clone group stays introduced. A group stays introduced also when every copy holds an added line, because the change then wrote the same text into each copy. Dead code, complexity, and duplication sections follow their respective schemas from the individual commands. Thresholds for complexity are inherited from `fallow health` config (defaults: cyclomatic 20, cognitive 15).

Audit creates a temporary git worktree to compare against the base ref. When the current checkout has `node_modules`, audit links it into the base worktree so tsconfig `extends` chains into installed packages and path aliases resolve like the working tree. The worktree is removed on normal exit. If the process is force-killed, run `git worktree prune` to clean up stale `.git/worktrees/fallow-audit-base-*` entries.
