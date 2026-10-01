# CLI internals

Use this reference for command parsing, orchestration, mutation, and rendering.
Use live `fallow --help` and generated contracts for the complete public
inventory.

## Ownership

- `crates/cli/src/main.rs` is a thin binary delegator.
- `crates/cli/src/lib.rs` owns Clap definitions, top-level dispatch, and the
  multicall surface.
- `crates/cli/src/check/`, `audit.rs`, `dupes.rs`, `health/`, `security.rs`,
  and `coverage/` translate CLI options into engine or API calls.
- `crates/cli/src/report/` owns terminal rendering and CLI format dispatch.
- `crates/cli/src/fix/` owns mutation planning and application.
- `crates/api/` owns reusable typed execution and output assembly.
- `crates/engine/` owns analysis, duplication, health, discovery, and
  command-neutral project state.
- `crates/output/` owns serialized report types and stable envelopes.

Analysis logic belongs below the CLI. A CLI module may validate arguments,
resolve paths, select an execution mode, render a result, and map the result to
an exit code.

A decision that more than one command or surface makes has one implementation.
The scope filters are `fallow_engine::dead_code::apply_scope`,
`fallow_engine::duplicates::apply_scope` and `fallow_engine::diff_scope`. The
change scope of a run is `fallow_engine::change_scope::ChangeScope`. Every
surface describes its inputs with a `ChangeScopeRequest` (the owner, whether a
global ref was requested, the changed files, and the run-wide
`PackageBaselineCache`) and calls `ChangeScope::resolve`, which owns the
precedence rule and the failure policy. The resolved value owns the result
filter, the `package_baselines` provenance rows, the `package-baselines`
request outcome, and the `scope_reason` that the `check` baseline
comparison, `--fail-on-stale-baseline`, and finding-id queries read. A surface
does not assemble these from separate decisions.

A changed-file set wins. A requested global ref gives the full scope when it
does not resolve, and it still suppresses `workspaces.changedSince`. A run whose
owner is `ChangeScopeOwner::Caller` never reads the package map: `audit` owns
the scope of its head and base runs, on the CLI and in the programmatic API, in
every production-mode split. The base snapshot may not be a Git repository,
and a package map that hid a base finding would report the head finding as
introduced.

A key of the package map is a workspace root exactly as discovery reports it,
which is what `fallow list --workspaces` prints. A symlinked workspace is
mapped under its link path. The nearest workspace owns a file; unlisted
workspaces and root files remain in full scope. Dependency-level findings
retain their existing global behavior, while manifest-owned findings follow
their owner path. The map scopes `check`, `dead-code`, `dupes`, and those
sections of a combined run. It does not scope `health` or `security`, alone or
in a combined run. A `--diff-file` or `--workspace` scope applies on top of the
map, so a finding must be in both.

The failure policy follows `--changed-since`. The config loader rejects a
malformed key (absolute, `..`, `.`, empty segment, trailing slash, backslash)
for every command, and a malformed ref fails the run with exit 2. A map that
cannot apply as written stands down as a whole: a key that names no workspace
of this project (for example in a run from a package subdirectory, which loads
the parent config), or a well-formed ref that Git cannot resolve (for example
in a shallow CI clone). The run then reports in full scope, writes a warning,
and publishes `request_outcomes["package-baselines"]` as `not-applied` with
the reason `unknown-workspace`, `git-failed`, `git-missing`, or
`not-a-repository`. A run that applies the map publishes the entry as
`applied`. The report is wider than asked, never narrower.

The analyses of one run share one resolution: the CLI keeps it in
`requests::package_baseline_cache`, and the programmatic API keeps it on the
call context. `check`, the API runtimes, and `dupes` resolve the map right after
the session discovers the workspaces, before the analysis, so a map error fails
fast. Git resolves each distinct ref once per run.

The scope always narrows the final result: the last scope filters run after
type-aware refinement and before baseline comparison and gates. `check` and the
programmatic dead-code runtime also narrow before refinement to save sidecar
work. They apply every scope filter again after refinement, because refinement
can add findings such as private-type leaks. The filters only remove findings,
so the second pass is idempotent for the findings that the first pass kept. The
editor narrows once, after refinement.

The global `--no-package-baselines` flag sets
`ChangeScopeRequest::no_package_baselines`, so the run never reads the map and
reports every package in full scope; the programmatic option, the MCP
parameter and the LSP initialization option `packageBaselines: false` set the
same field. `FALLOW_PACKAGE_BASELINES=false` empties
`ResolvedConfig::workspace_changed_since` at config load, in the engine for the
LSP, MCP and Node hosts and in `runtime_support.rs` for the CLI. A narrowed run reports the map as the
`package-baselines` scope reason, separate from `changed-since`, so a consumer
knows that dropping `--changed-since` does not widen it.

The saved baselines differ on purpose. `check` compares and saves its baseline
after the scope, so a baseline saved under the map is partial, records the
`package-baselines` scope reason, and prints a warning that names
`--no-package-baselines`. The file records the saving run's `scope_reasons`;
a later run that lacks one of them prints a warning before the comparison,
because it can report findings outside the saved scope as new. `dupes` compares its
baseline with the report before the package map narrows it, so the baseline
sees every clone group and no scope reason is recorded.

For example, `.fallowrc.json` can contain
`"workspaces": { "changedSince": { "packages/web": "main" } }`. JSON `check`,
`dead-code`, `dupes`, and combined reports include optional
`package_baselines` provenance when the map applies; SARIF, CodeClimate,
compact, Markdown, and badge output do not carry it. Each row has an exact
project-relative `workspace_root` and its `reference`. The rows list the
applied map, including packages that a `--workspace` scope does not report.
The field is absent for unconfigured runs, a global ref override, a map that
stood down, and `health`, `security`, and `audit` reports. The LSP publishes
the same row type in `fallow/analysisComplete`.

The production mode of each analysis is
`fallow_engine::project_config::ProductionFlags`. The error-severity rule is
`fallow_engine::error_severity`. The dead-code baseline loader is
`fallow_engine::baseline::apply_dead_code_baseline`. The editor complexity lens
is `fallow_engine::health::inline_complexity`. In the CLI, `crate::gates`
builds every `gate_outcomes` entry, and `crate::exit_codes::gate_exit_code`
maps a gate verdict to the exit code.

Exit 1 on an analysis command means that an enforced gate failed. A config
load warning never sets it. A workspace diagnostic such as
`node-modules-missing` or a broken tsconfig `extends` never sets it either.
The one exception is `source-parse-degraded`, which fails the run only through
the opt-in `--fail-on-parse-error` gate. Machine consumers read
`gate_outcomes`. Under `--quiet` and in every format except the human report,
`crate::gates::print_exit_reason` prints one stderr line that names the failed
gates, with `observed` and `threshold` when they exist. The text comes from
`report::gate_outcome_text::exit_reason_line`. The line leaves out the
`parse-error`, `stale-baseline` and `baseline-growth` gates, because each of
them prints its own line in every mode. The `own_lines` member of
`crate::gates::ExitReason` names the other gates that printed their own line in
this run, for example the regression outcome when the run is not quiet. The
line does not repeat them. A caller that owns the exit code, such as `audit`,
sets `exit_reason: false` on the section print options. The bare combined run
also sets it to false on each section, and prints one line for the whole run
after every gate set the exit code.

## High-value paths

- `crates/cli/src/audit.rs`: the CLI runners and the review brief of the
  changed-code audit. The audit itself (base snapshot, rename remap,
  attribution, dependency scope, verdict) is `crates/api/src/audit_run/`,
  which `fallow audit`, the MCP `audit` tool and `fallow_api::run_audit`
  share.
- `crates/cli/src/base_worktree.rs`: temporary base snapshots and cleanup.
- `crates/cli/src/check/`: dead-code filters, severities, workspaces, and
  baselines.
- `crates/cli/src/report/`: human and machine-readable rendering.
- `crates/cli/src/fix/`: dry-run plans and confirmed mutations.
- `crates/cli/src/agent_install/`: one-pass agent onboarding (`fallow agent`),
  composed over `setup_hooks.rs` and `init.rs`; `build.rs` embeds the shipped
  skill for installs without `node_modules/fallow`.
- `crates/cli/src/doctor.rs`: human and JSON rendering plus exit semantics for
  the read-only readiness report assembled by `crates/api/src/doctor.rs`.
- `crates/cli/src/coverage/` and `license/`: runtime coverage and license
  command orchestration.
- `crates/cli/src/viz/mod.rs`: the self-contained HTML map, plus the DOT and
  Mermaid text renderings of the import graph.
- `crates/cli/src/telemetry.rs`: local opt-in telemetry state and spooling.
- `crates/cli/src/cli_impact.rs` and `impact.rs`: local Impact history,
  attribution, aggregation, and the status-bar surface.
- `crates/cli/src/runtime_support.rs`: shared config and ownership helpers.

## Invariants

- Dead-code `finding_id` values are owned by `fallow_types::identity`. The
  engine pipeline (`run_engine_owned_dead_code_pipeline` in
  `crates/engine/src/session.rs`) stamps them once, after the detectors and
  before `ignoreFindings`, the scope filters, baselines and rule severities.
  The CLI, LSP, MCP and napi reach dead-code results only through that
  pipeline. Type-aware refinement runs after the scope filters, so it calls
  `stamp_missing_finding_ids`: it keeps each existing id and gives an id only
  to a finding without one. Do not restamp a filtered set, because the `~k`
  tiebreak suffix depends on the other findings with the same subject. The
  same module owns the FNV-1a 64 helpers that CodeClimate and SARIF
  fingerprints and security ids use; do not add another copy.
- A dead-code SARIF result has three `partialFingerprints` keys:
  `tools.fallow.fingerprint/v1` and `primaryLocationLineHash/v1` (rule, URI,
  normalized snippet and column; GitHub code scanning reads the second) and
  `fallowFinding/v1` (the `finding_id`). `append_sarif_findings` in
  `crates/output/src/sarif.rs` writes the third key, and only for a finding
  that gives exactly one result. The fan-out helpers for unlisted
  dependencies and duplicate exports do not write it.
  `ensure_unique_result_fingerprints` rewrites only the two location-based
  keys. Do not change their inputs: a change reopens every GitHub alert.
- The canonical key of a dead-code finding (`IdentifiedFinding::canonical_key`
  in `fallow_types::identity`) is the readable input of its `finding_id`:
  `<rule>:<path>:<name>...`, never a line or a suppression reason. The
  dead-code baseline (`crates/engine/src/baseline.rs`) and the audit new-only
  keys (`crates/api/src/audit_keys.rs`) use only this key, so the id, the
  baseline and the audit cannot drift. Do not build a dead-code key by hand.
  - A saved baseline carries `"identity": "dc1"`, stores the key once for
    each occurrence, and matches by count. A baseline without `identity` is a
    legacy file: the legacy filter matches each old entry exactly, and the
    legacy key builders stay only as test helpers. A legacy load prints a
    stderr note in human output and sets `baseline_staleness.format:
    "legacy"` in JSON. It never fails the run.
  - The audit numbers repeated keys with `dead_code_occurrence_keys` (`:~1`,
    `:~2`, in collection order), so the base and the head compare by count
    and the rename remap still sees the path as its own segment. An
    unlisted-dependency finding is the package, not an import site, so a new
    import site of a package that the base already reports stays inherited.
    A change to
    the audit key form must bump `AUDIT_BASE_SNAPSHOT_CACHE_VERSION` in
    `crates/cli/src/audit_cache.rs`.
- `dead-code --finding-id <id>` (repeatable or comma-separated) reports only
  the requested findings. `fallow_engine::dead_code::FindingIdFilter` owns the
  syntax check and the filter; `FindingIdTrace` owns the evidence. The CLI
  (`execute_check`) and `fallow_api::run_dead_code_with_baseline` use the same
  two types, so the answer is equal on every surface (drift invariant I11).
  The order is fixed:
  1. The trace records the requested ids on the full result set, before the
     scope filters.
  2. Scope, issue-type filters and rule severities run. The requested ids that
     disappear here go to `filtered`.
  3. Type-aware refinement runs outside a stage: a finding it removes is gone,
     not filtered.
  4. The baseline runs as a second filter stage.
  5. Regression and the baseline save see the set before the id filter.
  6. The id filter runs last. The SARIF side file, the JSON envelope and the
     error-severity exit code see only the requested findings.
- The JSON envelope carries `finding_id_query` only when the run received ids:
  `requested`, `found`, `missing`, `filtered`, `conclusive` and
  `inconclusive_reasons`. `conclusive` is false when the run used a scope
  channel (the same set as the baseline `scope_reasons`: diff,
  `--changed-since`, `--workspace`, `--changed-workspaces`, a positional path,
  `--file`, an issue-type filter, production mode, `includeEntryExports` from
  the flag or the config), `--baseline`, when the rule
  of a missing id is `off` in `rules` or in any `overrides[].rules`
  (`rule-off`), or when a requested id was filtered (`filtered`). Production
  mode counts whether it comes from the flag or from the project config, so a
  project with `production: true` in its config never gets a conclusive
  answer. A missing id under `conclusive: false` is unknown, never resolved.
- `finding_id_query.analysis_fingerprint` (`af1:<16 hex>`) is
  `fallow_engine::dead_code::analysis_fingerprint`. It hashes the fallow
  version, `ResolvedConfig::detection_config_digest` (the merged user config
  after `extends` without the keys in `NON_DETECTION_CONFIG_KEYS`, plus the
  loaded external plugins and rule packs, all as canonical JSON with sorted
  keys), the settings a surface changes after resolution (production mode,
  `includeEntryExports`, the effective rules, type-aware mode and requirement,
  type-aware project list, the file size limit) and the root-relative path and
  normalized content (CRLF to LF, trailing newlines removed) of these files:
  - every `.gitignore` and `.ignore` the walk reaches, and
    `.git/info/exclude`;
  - every `package.json`, `tsconfig*.json` and `jsconfig*.json`, plus the
    files a tsconfig `extends` chain names (relative, or a package under the
    root `node_modules`), also outside the walk;
  - every file that matches a config pattern of a built-in plugin
    (`fallow_core::plugins::registry::builtin_config_patterns`) or of an
    external plugin, whether or not the plugin is active this run.

  The approach is a declared file set, not tracking of the files the resolver
  and the plugin registry open: tracking would thread a recorder through the
  resolver and every plugin. The set is a superset of what a run reads, so
  the error is a false "unknown", never a false "resolved". The walk ignores
  the global git excludes file (`git_global(false)`), skips hidden
  directories (the fallow cache lives there), `node_modules` and
  `ignorePatterns` matches. Known exclusions: the global git excludes file and
  other machine environment outside the `FALLOW_*` variables. Source files
  are not inputs, so a source edit keeps the value; a manifest or tsconfig
  edit changes it, also when the edit fixes a dependency finding. `FindingIdTrace::finish` computes
  it from the resolved config, so every surface gives the same value.
- A consumer stores the fingerprint with its verdict. A later query with
  another fingerprint is unknown, even when `conclusive` is true: a config,
  ignore file, plugin or version change can hide a finding that still exists,
  and no reason list can see a change between two runs.
- A missing id under `conclusive: true` and an equal fingerprint means
  "fixed, suppressed, or ignored by config", never "unknown". An inline suppression comment or an
  `ignoreFindings` entry hides a finding because a person chose to hide it, so
  the finding counts as absent. A consumer that must tell a fix from a
  suppression reads the suppression state separately.
- The exit code follows the normal rule: 1 when
  a reported finding has error severity, 0 when every requested id is missing.
  There is no separate exit code for a missing id. A malformed id exits 2,
  because a typo must never read as "resolved".
- Health tie ordering and duplication collision handles are owned by the engine.
  Renderers, trace lookup, suppressions and baselines must use the same assigned
  handles. Preserve the [collision migration contract](../backwards-compatibility.md#report-ordering-and-colliding-duplication-handles)
  when changing report identity.
- Resolve user-provided file inputs against the user's project root before an
  audit switches to a base worktree. Prefix values such as `--coverage-root`
  remain absolute prefixes and must not be reinterpreted as input files.
- Audit base coverage attribution (#2347): the base-worktree pass scores from
  the same head-generated Istanbul map as the head pass, for explicit and
  auto-detected coverage alike. When no `--coverage-root` was given, the
  canonical head project root becomes the strip prefix so every recorded path
  rebases onto the base worktree; an explicit prefix is forwarded unchanged.
  Relocated base lookups (`coverage_relocated`) tolerate unbounded line drift
  only when every same-named entry in the file agrees on one value.
  Consequences: complexity growth in files the map reports as untested
  attributes as inherited once the base function also exceeds the threshold,
  and test deletions cannot surface as `introduced` complexity findings (the
  base is scored with the post-deletion map); coverage regressions belong to
  `rules.coverage-gaps` and health trends. The auto-detected map's content
  participates in the base-snapshot cache key exactly like `--coverage`.
- Audit coverage inputs (#2359) resolve through the same precedence as
  `fallow health` and bare `fallow`: `--coverage` / `--coverage-root`, then
  `FALLOW_COVERAGE` / `FALLOW_COVERAGE_ROOT`, then `health.coverage` /
  `health.coverageRoot`, then auto-detection.
  `fallow_api::coverage::resolve_coverage_inputs` is the single, pure owner of
  that order (#2368); `resolve_coverage_inputs` in `crates/cli/src/lib.rs`
  reads the env vars at the CLI boundary, loads config lazily, and delegates,
  and the MCP `audit` / `check_health` typed route delegates the same way. The
  resolved paths land in `AuditOptions`, so the head pass, the base-worktree
  rebase, and the base-snapshot cache key all see the same map. A configured
  path that does not exist fails audit with the same structured exit 2 as
  health, and a relative winning root is rejected before analysis starts.
  Resolution is scoped to runs that score health: bare combined mode resolves
  coverage only when `--only` / `--skip` keep health in the run, so
  `fallow --only check` neither loads config for coverage nor rejects a
  `health.coverageRoot` it never reads.
- Audit base-ref auto-detection is engine-owned.
  `fallow_engine::repo_refs::auto_detect_audit_base_ref` is the single owner of
  the upstream / remote-default / local-branch order, and both
  `crates/cli/src/audit_base_ref.rs` and the typed `audit` and
  `decision_surface` routes in `crates/api/src/runtime/` delegate to it. The
  CLI previously kept a second copy with its own git probe, which is how the
  two routes drifted into disagreeing about the same repository (#2699). Do not
  reintroduce base-ref detection, or a git probe serving it, outside
  `repo_refs`; every probe there returns trimmed, non-empty stdout, because
  callers feed the values back to git as refs and compare them as paths.
  The short HEAD SHA and the base analysis root are engine-owned for the same
  reason: `repo_refs::short_head_sha` and `repo_refs::base_analysis_root` are
  the single owners, and the CLI, the engine vital signs and the typed routes
  call them. `base_analysis_root` compares real paths on both sides, because a
  caller can spell the root through a symbolic link while git reports the
  resolved top level (#2740). Unrelated CLI-local probes stay where they are:
  the base-worktree helpers in `crates/cli/src/base_worktree.rs` and the hook
  scaffolding in `crates/cli/src/init.rs`.
- A base analysis root that the base commit does not contain is a normal audit
  shape, not a caller error. `repo_refs::resolve_base_analysis_root` reports it,
  and the typed `audit` and `decision_surface` routes take an empty base
  snapshot for it, so a package added on the branch has everything under it
  attributed as introduced, matching the CLI (#2699).
- Audit worktree cleanup must be scoped to Fallow-owned paths and registrations.
  Never prune unrelated user worktrees.
- `ci reconcile-review` and `ci post-review` isolate provider lifecycle
  failures per fingerprint: a failed mutation blocks only the remaining
  operations of that fingerprint, and every other stale fingerprint is still
  resolved in the same run. Both commands report `apply_hint`,
  `failed_fingerprints`, and `unapplied_fingerprints` so a wrapper can name what
  was left for the next run. Neither command creates or mutates a provider
  review; the content-free "reviewed" row that follows a resolution reply is
  GitHub's own wrapper around a standalone review-comment reply, and no
  available endpoint avoids it.
- Review comments close with a `fallow-fingerprint:v3:` marker. The dead-code
  fingerprint in it is the hash of the `finding_id`
  (`crates/api/src/dead_code_codeclimate.rs::dead_code_issue`), so it holds no
  line. `CodeClimateIssue::legacy_fingerprint` keeps the older line-based value
  in memory only (it is not serialized), and the review envelope publishes it
  per comment as `legacy_fingerprint`. The CI comment and review renderers
  must take typed CodeClimate issues: a round trip through the CodeClimate
  JSON drops the legacy value. `ci post-review` and `ci reconcile-review`
  read v1, v2 and v3 markers (`extract_fallow_fingerprint`) and treat an open
  lifecycle whose marker holds a comment's `legacy_fingerprint` as that
  comment (`envelope_legacy_fingerprints`, `reconcile_sets`). Remove the
  legacy field and this matching one release after the v3 marker shipped.
- JSON mode emits structured errors on stdout and keeps progress off stdout.
- Reported project paths remain relative unless an editor or protocol contract
  explicitly requires absolute paths.
- Serialized lists and human output use deterministic ordering.
- `--complexity-breakdown` is opt-in. Per-decision `contributions` are
  omitted by default and cloned only when requested; CLI and MCP health
  options forward the same choice to the engine findings builder.
- `fix` remains preview-first. Non-interactive mutation requires explicit
  confirmation.
- `fallow impact statusline` stays path-free, read-only, plain text, and
  epilogue-free. Its trend compares only whole-project scans.
- `fallow doctor` stays path-free, local, read-only, and epilogue-free. It may
  discover a trusted optional companion, but must not start it. Required
  readiness failures emit the complete report before exiting 2. Reports go to
  stdout; the command rejects `--output-file` so diagnostics cannot create or
  replace a project input.
- New output fields must move schemas, generated TypeScript contracts, MCP,
  LSP, VS Code, GitHub Action, and GitLab consumers together.
- New-only duplication demotion (issues #2164, #2220): under `--gate new-only`
  an introduced clone group none of whose instances overlap an added line is
  demoted to inherited. Without an opt-in shared diff, the merge-base worktree
  diff decides (`crates/api/src/audit_run/outcome.rs`,
  `demote_preexisting_dupe_introductions`). The CLI and the programmatic
  runtime run the same function.
  When an opt-in shared diff (`--diff-file`, `--diff-stdin`, or
  `$FALLOW_DIFF_FILE`) is active, it has already filtered the head duplication
  report with the same added-line overlap predicate. Every retained clone group
  therefore vetoes demotion, so a shared diff can prevent a demotion but cannot
  produce a rendered shared-source demotion note. Demoted groups stay counted
  as inherited and additionally surface via
  `attribution.duplication_demoted` and a per-group `demotion_reason` field;
  human output names the deciding diff source in the demotion note.
- Audit dependency scope: a dependency-level finding (unused, type-only,
  test-only or misplaced dependency, unused catalog entry) is in audit scope
  only when the changeset touches the manifest or catalog file that declares
  it. The rule applies to the head run and to the base snapshot
  (`scope_dependency_findings` in `crates/api/src/audit_run/scope.rs`).
  `--changed-since` on the other commands keeps every dependency finding.
- Narrowing requests report their own fate (issues #2687, #2688). Two channels
  can be asked for and refused: `--changed-since` and the opt-in shared diff.
  Both widen the report rather than failing the run, so the fact travels on the
  envelope as `request_outcomes` and not only on stderr, which `--quiet`
  removes entirely on the `$FALLOW_DIFF_FILE` channel. Two rules keep it
  honest. The print is quiet-gated and the RECORDING is not, so the object is
  identical with and without `--quiet`; and the sentence the envelope carries is
  the same string the stderr line prints
  (`ChangedFilesError::changed_since_message`, `DiffStandDown::message`), so a
  log a human read and a report a script read cannot state different remedies.
  The diff outcome lives in a sibling `OnceLock` beside `SHARED_DIFF`
  (`crates/cli/src/report/ci/diff_filter.rs`) rather than inside it: that
  cache's three states each carry a documented correctness argument, and a
  reporting concern does not belong inside a filtering decision. A diff that
  parsed but names no analyzable file reports `applied`, because the filter WAS
  applied over an empty scope. `fallow audit` records nothing: it exits 2 rather
  than widen, and it resolves its base ref through the non-printing
  `crate::check::try_get_changed_files` so the widening sentence cannot reach a
  run that produced no report. On the combined envelope the root is the only
  carrier, matching `workspace_diagnostics`.
- The object also carries `--sarif-file`, which produces a file BESIDE the
  report rather than narrowing it, so every entry publishes `affects`
  (`scope` / `artifact`) and every consumer selects on that rather than on a
  name. Without it the one sentence a consumer writes for the whole object
  reported a failed SARIF write as a run wider than requested, on the pull
  request, the merge request, the job summary and the Action's
  `requests-unapplied` output. The class is derived from the name inside
  `RequestOutcome::applied` / `not_applied`, so an entry cannot be filed under
  one name carrying another's class.
- The stand-down sentences name their candidate bases by relation to the
  analysis root (`the project root`, `the repository root (the project root is
  <offset> below it)`) rather than by absolute path, because `message` is a wire
  field and every other path-bearing member of a fallow envelope is
  project-root-relative. `requested` is the exception by design: it echoes what
  the user typed, which may be an absolute path they chose.
- `fallow_engine::codeowners::CodeOwners` is the single CODEOWNERS matcher.
  Every owner, section and owner-count lookup (`--group-by owner`, `--group-by
  section`, ownership signals, coverage owner counts, audit ownership) uses it.
  The patterns follow gitignore semantics. A pattern without a trailing `/`
  matches a file or a directory, and a directory match covers all paths below
  it. Thus one rule can compile to two globs, and `glob_rules` maps each glob
  index back to its rule index. Use `last_matching_rule` for a new lookup. Do
  not read a `GlobSet` match index as a rule index, because last-match-wins
  compares rule indexes. The globs use `literal_separator`, so `docs/*`
  matches the direct children of `docs` only.

## Health grouping stage order

`health --group-by` runs these steps in order:

1. The findings, hotspot and target stages apply `--top` to the project lists.
   When `--group-by` is active and `--top` removes entries, each stage also
   keeps the complete list (`group_findings`, `GroupUntruncatedLists`).
   Project targets use the truncated hotspots, as an ungrouped run does.
   Group targets use the complete hotspots.
2. `vital_data.rs` computes the project vital signs and loads the trend
   baseline once (`load_trend_baseline`, from `.fallow/snapshots/` or from
   `--trend-from`). The project trend compares against it.
3. `output_build.rs` builds the grouping. `bucket_paths` assigns every file,
   then the `--group` selector removes buckets before `build_group`, so the
   per-group vital signs and the duplication subset run only for kept groups.
   Parse, graph and churn work stay project-wide, because group metrics read
   project-wide signals.
4. `build_group` counts severities and hotspots before it applies `--top` to
   the group lists, and returns the group vitals for the snapshot and trend.
   Then `GroupListSections` empties each group list that the project report
   omits, with the same gates as `assembly.rs`. Thus a `--score` run keeps
   the score and counts of each group, but no group findings. The group
   duplication penalty counts only clone groups with two or more instances in
   the group, so a clone that spans two groups lowers only the project score.
5. `apply_group_trends` matches groups by key against the baseline, only for
   the same `grouped_by` mode. A group without a stored entry is
   `new_group` only when the stored `group_filter` keeps its key. Otherwise
   the baseline run did not measure it, and the group gets
   `no_group_baseline`.
6. `maybe_save_health_snapshot` saves the snapshot with the group data. It
   runs after the baseline load, so a trend never compares a run with its own
   snapshot.

Markdown and the GitHub job summary render the group table through one JSON
renderer (`fallow_api::build_health_groups_markdown`), so `report --from`
on a saved grouped envelope gives the same summary as the live run.

When a run does not list findings but `summary.functions_above_threshold` is
not zero (for example a `--score` run), the project complexity section of
both renderers gives that count and names `--complexity`. It does not say
that no function exceeds a threshold. The count comes before the baseline, so
a run with `summary.baseline_staleness` keeps the clean message: there, an
empty list means that the baseline accepts every finding.

## Compact health populations

Compact health output keeps the existing `vital-signs:` payload unchanged and
appends population rows immediately after it, in `functions`, `modules`, then
`templates` order:

```text
cyclomatic-population:functions:count=1,sum=1,max=1
cyclomatic-population:modules:count=1,sum=31,max=31
cyclomatic-population:templates:count=0,sum=0,max=null
```

Each row mirrors the matching `vital_signs.cyclomatic_population` JSON group.
Counts and sums are decimal integers; `max` is a decimal integer or literal
`null`. A measured-empty group has `count=0,sum=0,max=null`. Older reports without
population metadata emit no population rows. Sum the group sums and counts to
reconstruct the cyclomatic mean; module scopes remain aggregate-only and do not
produce function findings. These rows describe metrics, not findings.

## Viz lenses and availability

`fallow viz` runs one engine-owned project analysis with complexity artifacts
and the graph retained, then feeds `fallow_engine::viz::VizData` into a
self-contained HTML shell. `render_html` inlines the CSS, the frontend bundle,
and the payload into one file, so producer and consumer always ship together
and the payload carries no version of its own. The prebuilt TypeScript
frontend lives in `viz-frontend/` and is embedded from
`crates/cli/viz-assets/viz.js` and `viz.css`; rebuild it with
`cd viz-frontend && npm ci && npm run build` rather than editing the bundle.

The payload travels in `type="application/json"` script tags, not as a
script literal. `crates/cli/src/viz/payload.rs` writes the core (files,
edges, summary and each availability) into `#fallow-data`, and each large
finding list into its own `data-fallow-lazy` tag. The per-file function
lists travel as one column that aligns with `files`. The frontend
(`viz-frontend/src/payload.ts`) parses a lazy section on the first read of
its property, so a lens that is not open costs no parse. The lens indexes in
`buildIndex` are computed on first use for the same reason. Arrays of
objects travel as `{"$k", "$r"}` tables to cut repeated keys; both sides pin
one fixture for this encoding. Code that the first paint runs must not read
a lazy section, or the deferral is lost.

The page body starts with a static frame from `viz-frontend/src/shell.html`,
which the frontend build copies to `crates/cli/viz-assets/shell.html`. It has
the page rows (top bar with the project name, toolbar, context strip, stage and
status line) with the ids in `viz-frontend/src/shell.ts`, so each row takes the
same box before and after the script replaces the frame. Keep counts and other
analysis text out of the frame: only the script derives them.

Invariants:

- Every analysis family carries an availability state (complete, disabled, not
  applicable, unavailable) alongside a unit-labelled count. A family that did
  not run must render as missing data, never as zero findings. Only the
  complete state licenses reading the count.
- The Health lens reuses the retained artifacts instead of reparsing. Churn,
  hotspots, and ownership need a git-history walk viz does not perform, so
  they report as unavailable rather than as empty. Runtime evidence needs a
  runtime coverage input viz does not take, so the `runtime` capability
  reports unavailable with that reason instead of letting a complete static
  answer read as a complete one.
- Coverage for the Health lens resolves through the shared precedence order
  described under the audit invariants, reusing the config viz already loaded.
  Viz has no coverage flags of its own, and auto-detection stays in the engine.
- The Security lens enables the advisory security rules through the shared
  `enable_security_rules` helper and reports static candidates as candidates
  only. Runtime security stays explicitly unavailable without runtime
  evidence.
- The rule and finding identifiers shown in the Security lens come from
  `fallow-security`, the same source as the JSON `finding_id` and the SARIF
  `partialFingerprints` value, so the surfaces join and cannot drift.
- `--viz-format dot` and `--viz-format mermaid` render the import graph alone.
  They skip the Health run, the feature-flag pass, and the security rules,
  because none of that reaches their output.

## Audit cache maintenance

`fallow audit-cache` maintains the reusable base-snapshot caches
(`$TMPDIR/fallow-audit-base-cache-*`) that `fallow audit` builds and
garbage-collects. Two subcommands with distinct semantics:

- `audit-cache remove`: delete every cache owned by an explicit `--root`,
  warm or not. Requires `--root` and `--yes` (non-interactive), exits 2 on
  incomplete removal because it promises completeness.
- `audit-cache prune`: apply the same GC policy every audit run applies
  silently (orphaned-sidecar cleanup, age-based reclaim, cross-repo reclaim
  of abandoned entries), report every considered entry with sizes, and exit
  0 whenever the command ran, including lock-contention skips and per-entry
  failures. Machine consumers gate on the envelope's `complete` field.
  Defaults to the current directory as root. A pre-#1815 registration at the
  current cache path is only deregistered and stays warm on disk: it reports
  as kept with reason `legacy-deregistered`. The envelope's `deregistered`
  field is an informational subset of `kept`, not a fifth member of the
  `removed + kept + skipped + failed == found` partition. It also appears in
  the matching human summary line and never adds its size to
  `reclaimed_bytes`. Released SHA-keyed registrations are genuinely removed
  (reason `legacy-registered`) and stay counted as reclaimed.

Shared invariants (`crates/cli/src/base_worktree.rs`):

- Both prune modes and the per-audit sweep share one decision code path
  (`sweep_reusable_caches_with_report`), so prune can never drift from what
  audits actually reclaim.
- `--dry-run` performs zero filesystem mutation: no cache removal, no
  `.lock` sidecar creation (acquiring a lock would create one), no
  `.last-used` grace seeding, and no git worktree deregistration. The legacy
  registered-cache pass is still enumerated read-only with the same
  `legacy-registered` / `legacy-deregistered` split an apply run reports.
- `.lock` sidecars are permanent lock identities and are never deleted:
  removing an unlinked-but-still-flocked inode while a racer re-creates the
  path would split one lock across two inodes.
- Owner liveness for foreign entries is a NotFound-only probe on
  `std::fs::metadata`: only a definitive NotFound classifies the recorded
  owner root as dead. Every other probe error (EACCES, EIO, ENOTDIR) keeps
  the entry as `owner-unverifiable`, so a transient failure can never
  reclaim a live repo's cache or defeat its `cacheMaxAgeDays: 0` policy. A
  path below an unmounted mountpoint still reads NotFound and is not
  protected. A dangling-symlink owner root resolves NotFound (dead).
- Threshold precedence: `--max-age-days` flag, then
  `FALLOW_AUDIT_CACHE_MAX_AGE_DAYS`, then `audit.cacheMaxAgeDays`, then the
  30-day default. `0` disables age-based reclaim but still reclaims
  orphaned sidecars and dead-owner entries.
- Per-entry GC diagnostics are debug-level tracing shared by the audit sweep
  and prune: `RUST_LOG=fallow=debug fallow audit ...` (or any prune run)
  emits one `audit cache sweep considered entry` line per candidate with
  path, pass, mode, decision, reason, age, threshold, and owner fields. With
  `RUST_LOG` unset, audit stderr is unchanged.
- Prune entry sizes come from a plain recursive walk that never follows
  symlinks and deliberately ignores gitignore semantics: a cache entry is a
  checked-out snapshot whose `.gitignore` would otherwise hide
  `node_modules`, the bulk of the measurement.

## Verification

Start with focused CLI tests for the changed command. For output or schema
changes also run:

```bash
npm run generate:contracts:check
cargo test -p fallow-cli
npm run verify:fast
```

Run the matching format or integration review skill when a public rendering
surface changes.
