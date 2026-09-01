---
paths:
  - "crates/cli/**"
---

# fallow-cli crate

Key modules:
- `main.rs` — CLI definition (clap) + command dispatch
- `error.rs` — Structured error output (`emit_error`): JSON on stdout when `--format json`, stderr otherwise
- `audit.rs` — Audit command: combined dead-code + complexity + duplication for changed files, verdict (pass/warn/fail)
- `check.rs` — Analysis pipeline, tracing, filtering, output
- `dupes.rs` — Duplication detection, baseline, cross-reference
- `health/` — Complexity analysis: `mod.rs` (orchestration), `scoring.rs`, `hotspots.rs`, `targets.rs`
- `watch.rs` — File watcher with debounced re-analysis
- `fix/` — Auto-fix: `exports.rs`, `enum_members.rs`, `deps.rs`, `io.rs` (atomic writes)
- `codeowners.rs` — CODEOWNERS file parser, ownership lookup for `--group-by owner`
- `report/` — Output formatting: `mod.rs` (dispatch), `grouping.rs` (ownership resolver, result partitioning), `human/` (check, dupes, health, perf, traces), `json.rs`, `sarif.rs`, `compact.rs`, `markdown.rs`, `codeclimate.rs`
- `migrate/` — Config migration from knip/jscpd
- `init.rs` — Generate config files (`.fallowrc.json` or `fallow.toml`), scaffold pre-commit git hooks (`--hooks`)
- `list.rs` — Show active plugins, entry points, files, boundary zones/rules (`--boundaries`)
- `schema.rs` — `schema`, `config-schema`, `plugin-schema` commands
- `viz.rs`: the `viz` command, one analysis session rendered as a self-contained HTML map (inlined CSS, the prebuilt `viz-frontend/` bundle from `crates/cli/viz-assets/`, and the JSON payload in one file), or the import graph alone as DOT/Mermaid. Every analysis family in the payload carries an availability state (complete, disabled, not applicable, unavailable) with a unit-labelled count, so a family that did not run renders as missing data instead of as zero findings. The text formats skip the health run, the feature-flag pass, and the security rules, none of which reach their output. Coverage resolves through the shared `resolve_coverage_inputs`; `viz` has no coverage flag of its own
- `security.rs` - opt-in `fallow security` command surfacing local security CANDIDATES, not verified vulnerabilities. MVP rule `client-server-leak` lives in `crates/core/src/analyze/security/mod.rs`. `run()` loads config via `load_config_for_analysis`, forces `rules.security_client_server_leak` from `off` to `warn` while respecting an explicit user `error`, runs `fallow_core::analyze`, applies `--workspace`, `--changed-since`, and shared `--diff-file` filtering, relativizes finding and trace paths, and renders `SecurityOutput` as human, JSON, or SARIF. `--ci`, `--fail-on-issues`, `--sarif-file`, and `--summary` follow the normal CLI contract. SARIF is hand-built at `level: note` with `partialFingerprints` and no CWE; trace hops become `relatedLocations`. JSON uses the typed `FallowOutput::Security` root with `kind: "security"` (tagged root envelopes are the only wire shape since the `--legacy-envelope` removal in v2.104.0). Findings are `#[serde(skip)]` on `AnalysisResults`, so they never appear under bare `fallow` or `audit`.
- `config.rs` — `config` subcommand: prints loaded config path + JSON resolved config (or `--path` only). Honors global `--config <path>`.
- `explain.rs` — Metric/rule definitions, JSON `_meta` builders, SARIF `fullDescription`/`helpUri` source, docs URLs
- `validate.rs` — Input validation (control characters, path sanitization)
- `regression/` — Regression testing: `tolerance.rs` (thresholds), `counts.rs` (baselines), `outcome.rs` (verdict), `baseline.rs` (save/load/compare)

## Environment variables
- `FALLOW_FORMAT` — default output format
- `FALLOW_QUIET` — suppress progress bars
- `FALLOW_BIN` — binary path for MCP server
- `FALLOW_COVERAGE`: path to Istanbul coverage data for accurate CRAP scores; honored by `health`, bare `fallow`, `audit`, and `viz`
- `FALLOW_COVERAGE_ROOT`: absolute prefix stripped from Istanbul paths before CRAP matching; same honoring surfaces as `FALLOW_COVERAGE`
- `FALLOW_CA_BUNDLE` — PEM trust bundle for fallow cloud and provider HTTP calls. Relative paths resolve from the process cwd. The bundle replaces default WebPKI roots, so private-CA setups need a complete bundle.

## JSON error format
Structured JSON errors on stdout when `--format json` is active: `{"error": true, "message": "...", "exit_code": 2}`
- **`--complexity-breakdown`**: opt-in flag adding a per-decision-point `contributions[]` array to each complexity finding in `--format json`; threaded through `HealthOptions.complexity_breakdown` into `collect_findings` + `merge_crap_findings` (clone `FunctionComplexity.contributions` only when set; default off; omitted via `skip_serializing_if`). Drives the VS Code inline breakdown; MCP `check_health` param `complexity_breakdown`.
