# CLI internals

Use this reference for command parsing, orchestration, mutation, and rendering.
Use live `fallow --help` and generated contracts for the complete public
inventory.

## Ownership

- `crates/cli/src/main.rs` is a thin binary delegator.
- `crates/cli/src/lib.rs` owns the run entry point and the multicall surface.
- `crates/cli/src/cli_args.rs` owns the Clap definitions.
- `crates/cli/src/dispatch.rs` owns subcommand dispatch.
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
three dead-code scope filters share one visitor, `retain_in_scope` in
`crates/engine/src/result_scope.rs`, and each scope is a `ResultScope` type.
The change scope of a run is `fallow_engine::change_scope::ChangeScope`. Every
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

`fallow_engine::workspace_scope::WorkspaceScope` decides if a file is in the
scope of `--workspace`, `--changed-workspaces` and a positional path. A file is
in scope when the deepest selected or excluded root that holds it is a selected
root. A `!` pattern thus removes a nested workspace from a selected parent. A
nested workspace that the selection does not name stays in scope. New code
calls `WorkspaceScope::contains` and does not test a prefix of `roots()`.

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

Each config loader sets the `production` value of one run with
`ProductionConfig::for_run`, which sets the mode for every analysis and keeps
the `production.ignore` globs of the object form. `ResolvedConfig` stores the
globs in `production_ignore`. Discovery and the watch filter add them to the
built-in production patterns only when the run is in production mode, so
`--production` and `FALLOW_PRODUCTION` also apply them. The globs apply to each
analysis that runs in production mode, not only to dead-code analysis.

### Config loader adapter

The CLI reads each config file through
`fallow_engine::project_config::load_config_file`. The source rule
`config-load-through-engine-loader` in the root `Cargo.toml` denies a direct
load. `runtime_support::load_config_for_analysis` does these steps:

1. It loads the file through the engine loader.
   `runtime_support::config_load_error_message` gives the error text, and the
   error exits 2. An explicit file adds `failed to load config '<path>': ` to
   the top error. The top error of a discovered file names the path itself.
2. It prints `loaded config: <path>` on stderr, once for each path, in human
   mode only, and not under `--quiet`.
3. `ConfigFile::for_analysis` flattens the `production` setting for the
   analysis, with `--production` as the global flag.
4. It records the config shape for telemetry.
5. `ConfigFile::validate` checks the external plugins, the boundaries and the
   rule packs.
6. `ConfigFile::resolve` resolves the config and applies the environment
   variables that every host honors, such as `FALLOW_MAX_FILE_SIZE` and
   `FALLOW_CACHE_MAX_SIZE`. The `--max-file-size` flag then replaces the
   file size limit.
   For a root in a temporary worktree of `fallow audit`, the cache
   directory that `runtime_support::set_root_cache_dir_override` registered
   then replaces the resolved one (see
   [Parse cache of the audit worktrees](#parse-cache-of-the-audit-worktrees)).
7. It records the cache notice candidate, reports the workspace diagnostics
   and warns about unknown security categories.

The bare run, `audit`, `decision-surface`, `dead-code`, `architecture`,
`dupes`, `health`, `list`, `workspaces`, `fix`, `watch`, `viz`, `flags`,
`suppressions`, `trace`, `trace-error`, `security`, `inspect`, `coverage
analyze`, the coverage uploads, `guard`, `rule-pack list` and `rule-pack test`,
`baselines prune`, `plugin-check`, `mutation` and `similar-code` resolve one
run instead:

1. `run_inputs::command_run_inputs` maps the parsed command line to
   `fallow_config::RunInputs`: the `CommandKind`, the selection of `--only`
   and `--skip`, the global flags and the production flags that the command
   reads. `main` captures one `EnvSnapshot` for the inputs.
2. `DispatchContext::begin_run` calls
   `fallow_engine::resolved_run::resolve_run` once, at the point where the
   command parsed its config. The resolver loads and validates the config
   before the first analysis, decides the selection and the production mode
   of each analysis, and resolves one config for each production mode. A
   resolve error exits 2 with the text of the config loader. A run that selects
   no analysis keeps the CLI text of the bare run and of `audit`.
3. `DispatchContext::finish_run` applies the side effects of the load once,
   through `runtime_support::on_run_resolved`: the `loaded config` line, the
   config shape for telemetry, the cache notice candidate, the workspace
   discovery diagnostics and the security category warning. The notice counts
   the diagnostics of workspace discovery only, not the undeclared workspaces.
   `dead-code`, `dupes` and `health` call it after their flag checks, so a flag
   usage error prints before the `loaded config` line. A config that fails
   validation and a failed workspace discovery also exit 2 there.
   `DispatchContext::resolve_run` does both steps at once for `audit`. The bare
   run needs the plan of the run, so `DispatchContext::checked_run` stops it on
   a validation error before the flag checks.
4. The command reads its plan, its production modes, its cache directory and
   its coverage config from the run. Each analysis takes the resolved config of
   its production mode from the run through the `run` field of `CheckOptions`
   and `DupesOptions`, and through the `*_in` variants of the health entry
   points. The dead-code session opens with `Runnable::open_session_adjusted`,
   so a command setting cannot change a decided field.
5. `list`, `fix`, `viz`, `flags` and `suppressions` call
   `DispatchContext::begin_run` in the dispatch, and take a
   `cli_production::DeadCodeRun`. The command calls `DeadCodeRun::runnable`
   at the point where it loaded its config, so its own checks keep their
   order before the `loaded config` line. `list` applies the side effects with
   `--quiet` on, as it did before. `viz` prints a config error as text. A
   session of these commands opens through `cli_production::open_session`,
   so duplication in `viz` runs on the files of the duplication production
   mode. `watch` takes `DispatchContext::resolve_run` as its `resolve` step,
   and resolves the run again on each config reload.
6. The trace commands, `guard` and `rule-pack` take
   `cli_production::ResolveRun`, a borrowed `|| dispatch.resolve_run()`, and
   call it after their own checks of the command line. `security`,
   `baselines prune` and `plugin-check` resolve in the dispatch. `inspect`
   resolves for its churn cache directory and for the type-aware evidence of a
   symbol, and passes `--allow-remote-extends` to each child process.
   `coverage analyze` and the uploads resolve through `coverage::RunContext`,
   which clones the run inputs of the command line. Without `--production`,
   `coverage analyze` reads the production mode from the config. `mutation`
   and `similar-code` run through `fallow-api` with the host of
   `run_inputs::cli_host`, so their runs resolve as `Surface::Cli`.
7. The base side of `fallow security --gate newly-reachable` resolves with
   `fallow_engine::resolved_run::resolve_base_run` and `BaseConfig::BaseCopy`,
   so it reads the config file of the base commit.
8. A benchmark hook or a unit test without a command line resolves the run of
   `cli_production::benchmark_run`: the root with the discovered config, no
   parse cache, JSON output and no progress.

The sub-analyses of `audit` and its base worktree still load the config
through `runtime_support::load_config_for_analysis`. A command option struct
with `run: None` (a benchmark hook or a unit test) also loads the config
there. A site
that reads one optional setting, such as the audit cache, `fallow report` or
the staged audit, uses `runtime_support::loaded_config`. That function gives
`None` when the file does not load, and the site prints nothing.

A command that loads the config outside the analysis loader gets the
`--config` path and the `--allow-remote-extends` policy together as one
`runtime_support::ConfigFlags` value. `fallow report --from` and the staged
audit use it, so each load of one command
reads the same file with the same trust policy. The own-config pass of
`fallow audit` reads the base version of the `--config` file. When the change
added that file, discovery in the base worktree decides.

Exit 1 on an analysis command means that an enforced gate failed. A config
load warning never sets it. A workspace diagnostic such as
`node-modules-missing` or a broken tsconfig `extends` never sets it either.
The exceptions are `source-parse-degraded` and `source-nesting-too-deep`, which
fail the run only through the opt-in `--fail-on-parse-error` gate. Machine consumers read
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

A bare machine run without `--fail-on-issues` publishes the
`FAIL_ON_ISSUES_GATES` entries with `enforced: false` and exits 0 on them.
When such a run exits 0 and one of these entries fails, the combined printer
prints a `[W] Exit code 0:` line from
`report::gate_outcome_text::unenforced_gates_line`. The line names the gates,
the `--format` value and `--fail-on-issues`. It is printed also under
`--quiet`. A run that exits 1 for another gate does not print it, because the
line states exit code 0.

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
- `crates/cli/src/cli_impact.rs`, `impact.rs` and `impact_store.rs`: local
  Impact history, attribution, aggregation, and the status-bar surface. Each
  change of a project store goes through `transact` in `impact_store.rs`.
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
  dead-code baseline (`crates/engine/src/baseline/mod.rs`) and the audit new-only
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
    `crates/cli/src/audit_cache.rs`. The cache key comes from the inputs of
    the base pass. `BaseRunInputs` in `crates/cli/src/audit_base_run.rs`
    holds the inputs that the base pass and the key share, and the key reads
    the options that `BaseRunInputs::options` builds. An option that the
    base pass reads from the head options must be a named key field. For
    example, only a `--brief` base pass computes the public-export set, so
    `brief` is a key field. The key also holds each coverage file that the
    coverage reader selects
    (`fallow_engine::health::coverage_input::selected_coverage_files`) and
    the effective type-aware selection.
- The unused-file cascade filter is
  `fallow_engine::dead_code::hide_cascade_findings`. It runs inside
  `apply_rule_severities`, so the CLI, the programmatic API, MCP, the Node
  bindings and the editor diagnostics hide the same set. `fix` calls it
  directly, because `fix` runs no rule pass. The filter removes
  `unused_exports`, `unused_types`, `unused_class_members` and
  `unused_enum_members` findings only when the report lists their file in
  `unused_files`. Other finding kinds in an unused file stay.
  - An inline `fallow-ignore-file unused-file` comment removes the file from
    the report, and the filter still hides the export and member findings of
    that file. The detector records such files in
    `CascadeState::suppressed_unused_files`. A suppression comment therefore
    never adds a finding (drift invariant I6).
  - An issue-type selection without unused files, a scope, an
    `ignoreFindings` pattern or an `off` rule that removes the file from the
    report hides nothing in that file. The issue-type filters clear
    `suppressed_unused_files` too. The CLI therefore runs the issue-type
    filters before the rule pass, as the programmatic runtime does.
  - With `--changed-since` or `--workspace`, `cascade_hidden` counts only the
    findings in the scoped files, because the scope runs before the filter.
  - The removed findings move to `CascadeState::hidden`, and
    `AnalysisResults::cascade_hidden` counts them.
  - SARIF writes the count as `cascadeHidden` in the run `properties`
    (`fallow_output::dead_code_sarif`), and markdown adds one note line
    (`fallow_api::build_markdown`). Both are absent when the count is zero.
    CodeClimate, compact, GitHub annotations and grouped markdown do not
    carry the count.
  - The grouped envelope copies `cascade_hidden` to its root, as it does
    with `unused_load_data_keys_global_abstain`. The groups are new result
    sets, so no group carries the count.
  - The baseline runs after the filter. A baseline entry for the unused file
    does not make its findings visible again. A baseline entry that matches a
    hidden finding counts as matched in `baseline_staleness`, and
    `baselines prune` keeps it. A baseline saved while findings are hidden
    does not contain them.
  - A regression baseline (`--save-regression-baseline`) saved by an older
    version counts the hidden findings. After the upgrade it has more
    headroom, so users must save it again.
  - `fallow_engine::health::shared_parse_data_from_artifacts` restores the
    hidden findings before the health pipeline reads the dead-code results.
    The vital signs, the file scores and the score therefore count them in
    bare `fallow` and in standalone `health` alike.
  - `--show-cascade`, the `showCascade` config key and the MCP and Node
    `show_cascade` option turn the filter off.
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
  `includeEntryExports`, `showCascade`, the effective rules, type-aware mode and requirement,
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
- New-only estimated CRAP: under `--gate new-only`, `audit_run::attribute`
  calls `demote_estimated_crap_gates` (`crates/api/src/audit_complexity.rs`)
  before the comparison. A complexity finding that exceeds only CRAP, with
  `coverage_source` `estimated` or `estimated_component_inherited`, gets
  `effective_severity: warn`. The finding stays in the report, and SARIF,
  GitHub annotations and the verdict read the gate severity from that field.
  A finding that also exceeds cyclomatic or cognitive complexity keeps the
  severity of its rules. Measured coverage (`istanbul`) keeps the gate. In
  `exceeded_metric_increased`, a CRAP score from estimated coverage does not
  make a finding introduced, and a base finding without a CRAP score is not
  growth. `--gate all` keeps the rule severity.
- Moved functions under new-only: the identity key of a complexity finding
  holds the path, and git reports a rename only for a whole file. A refactor
  that splits a file gives each moved function a new key.
  `classify_complexity_attribution_with_moves`
  (`crates/api/src/audit_complexity.rs`) then gives a head finding without a
  base finding the base finding of the same function name from another
  changed or deleted file. The base health pass covers the whole project and
  the head pass covers the changed files, so `audit_run::compare` passes the
  changed files: a base finding in an unchanged file is no candidate. Equal
  values pair first. One remaining head finding and one remaining base
  finding of a deleted file pair and compare their metrics, so a moved
  function that grew is worsened. The pair needs a deleted file: in a file
  that still exists, a base finding without a head finding can be a function
  that the change made simpler, and a new function of the same name must not
  take it. With more candidates of one name, the remaining head findings are
  introduced. A placeholder name (`<arrow>`, `<anonymous>`)
  needs equal cyclomatic and cognitive values and an equal line count.
- Moved unused code and moved clone groups under new-only:
  `crates/api/src/audit_moved.rs` finds the base key that a moved finding
  left behind, and `audit_run::compare` then makes the head finding
  inherited. A base key is a candidate when no head finding has it and its
  path is a changed or deleted file. Each base key covers one head finding,
  so a copy of moved code is introduced.
  - Dead code: the base key has the same rule and the same parts as the head
    key, with another changed path. Only the rules in `MOVABLE_RULES` take
    part: their key names a symbol that the file declares. A dependency, a
    suppression comment or a boundary target is not code of the file, so
    such a finding never matches. The diff of the run must add a line with
    the symbol to the head file. Without that line, the change made existing
    code unused, and the finding stays introduced. When two findings compete
    for one base key, the finding with the lower severity is inherited. The
    step does not compare the code.
  - Clone groups: the base key has the same token count, line count and
    fragment hash as the head key, under other files that are all changed
    paths. The fragment hash covers the text of each instance and does not
    depend on the instance order, so only a group with unchanged text
    matches.
- Audit unused code outside the changed files: under `--gate new-only`, the
  head and base dead-code passes keep every unused file and unused export.
  They pass `ChangeScopeOwner::CallerKeepingUnusedCode`
  (`crates/engine/src/change_scope.rs`), so the change scope keeps these
  findings and the run still counts as change-scoped for a baseline. The CLI
  then calls `filter_results_by_changed_files_keeping_unused_code` in
  `crates/engine/src/changed_files.rs`.
  `audit_run::attribute` calls `scope_unchanged_unused_code`
  (`crates/api/src/audit_run/scope.rs`): such a finding outside the changed
  files stays only when the base snapshot does not report its key. The base
  pass uses the head config, so a removed entry point is not visible there.
  When the change touches the config file of the head run,
  `base_own_config_unused_code` (`crates/cli/src/audit_dead_code.rs`) runs
  one more syntactic dead-code pass with the config of the base commit and
  stores its unused-code keys in `AuditKeySnapshot::own_config_unused_code`.
  `own_config_unused_code_keys` (`crates/api/src/audit_run/own_config.rs`)
  also adds each unused file or unused export of the base pass with the head
  config that the base config ignores (`ignorePatterns`, `ignoreFindings`,
  `ignoreExports`, or an override that turns the rule off), and each unused
  export in a file that the base config reports as an unused file. The base
  config cannot prove that such code was in use. `config_path_under_root`
  canonicalizes the config path and the root, so a relative `-c` path or a
  path through a symlink still starts the pass.
  `forget_unused_code_used_under_own_config` then removes each head key that
  this set lacks from the base keys. The set is `None` when the base config
  does not load or turns `unused-files` or `unused-exports` off, so a rule
  that the change turns on does not gate old findings. The programmatic
  backend gives the same verdict: its audit sections use the same change
  scope owner, and its base pass runs the same second pass with the base
  config (`crates/api/src/runtime/audit_own_config.rs`).
- Audit test-file complexity: under every gate, `audit_run::attribute` calls
  `exempt_test_file_gates` (`crates/api/src/audit_complexity.rs`) before the
  comparison. It reuses `fallow_engine::test_paths::is_test_path` on the path
  below the project root, so specs, test directories, end-to-end suites, mocks
  and fixtures match. Each such finding gets `effective_severity: warn` and
  `gate_exemption: test-file`. The attribution does not change. The finding
  stays in the report. SARIF puts `gate_exemption` in the property bag at
  level `warning`, CodeClimate appends a note to the description, and the
  GitHub summary shows `warn (test file)`. The human, markdown and compact
  views list the test-file findings that would otherwise take part in the
  gate (`crates/cli/src/audit_complexity_notes.rs`). `fallow health` does not
  call this function.
- New-only growth of an exceeded metric: `ComplexityClass::of_match`
  (`crates/api/src/audit_complexity.rs`) makes a matched finding introduced
  only in two cases. In the first case, the head finding exceeds a metric that
  the base finding did not exceed, and that metric grew. In the second case,
  a metric that the head finding exceeds grew, and the severity tier
  (`moderate`, `high`, `critical`) is higher than in the base finding. Other
  growth is `worsened`. `ComplexityMetrics` stores the exceeded flags, the
  severity tier and the coverage source of the base finding from the base run.
  Do not recompute them with the head thresholds, because a
  `health.thresholdOverrides` change in the same PR would then make an old
  finding introduced. The tiers use the fixed `DEFAULT_*_HIGH` and
  `DEFAULT_*_CRITICAL` limits. CRAP takes part in the tier only when both sides
  have a measured score. `audit.worsened: error` is the strict mode for teams
  that want every growth to gate. `audit.worsened: off` clears growth only in
  metrics that the finding does not exceed. Growth of an exceeded metric stays
  `worsened` and gives at least `warn` (`clear_worsened_for_off`), because
  before this rule it made the finding introduced. The audit base snapshot
  cache is version 15.
- Audit coverage inputs (#2359) resolve through the same precedence as
  `fallow health` and bare `fallow`: `--coverage` / `--coverage-root`, then
  `FALLOW_COVERAGE` / `FALLOW_COVERAGE_ROOT`, then `health.coverage` /
  `health.coverageRoot`, then auto-detection.
  `fallow_api::coverage::resolve_coverage_inputs` is the single, pure owner of
  that order (#2368); `resolve_coverage_inputs` in `crates/cli/src/dispatch.rs`
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
- An audit review comment on a complexity, styling, dead-code,
  stale-suppression or duplication finding puts the attribution after the
  severity label, and says when the gate does not count the finding:
  `` **error** · inherited · not gated (`--gate new-only`) ``. The health
  CodeClimate builder copies the attribution and the gate exemption into
  `CodeClimateIssue::audit` (in memory only, like `legacy_fingerprint`). The
  dead-code builder reads the `introduced` flag of each finding. The
  duplication issues get the flag of their clone group from
  `set_duplication_audit_attribution`; for a saved audit, the flags come from
  the `introduced` field of each saved clone group. The typed stale
  suppression has no audit flag, so `set_stale_suppression_audit_attribution`
  sets it from the base snapshot for a live run, and from the `introduced`
  field of each saved stale suppression for a saved audit. The
  review renderer reads the gate policy from `AuditResult::attribution` for a
  live run, and from the `attribution` object of a saved audit envelope
  (`AuditGatePolicy::from_audit_envelope`). `--gate all` has no attribution,
  so the label stays plain.
- JSON mode emits structured errors on stdout and keeps progress off stdout.
- Reported project paths remain relative unless an editor or protocol contract
  explicitly requires absolute paths.
- Serialized lists and human output use deterministic ordering.
- `--complexity-breakdown` is opt-in. Per-decision `contributions` are
  omitted by default and cloned only when requested; CLI and MCP health
  options forward the same choice to the engine findings builder.
- `fix` remains preview-first. Non-interactive mutation requires explicit
  confirmation.
- Source edits of `fallow fix`: each source fixer (class members, exports,
  enum members) reads a file through `read_source_snapshot`
  (`crates/cli/src/fix/source_edits.rs`) and gets the text that the analysis
  read, also after another fixer queued edits for the file. The fixer queues
  byte edits against that text, and `FixPlan::commit` applies the edits of a
  file one time. An edit that overlaps a queued edit is not applied and gets
  no fix record. The fixers of `package.json`, `pnpm-workspace.yaml` and the
  config file still stage whole files.
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
  as inherited and are also reported through
  `attribution.duplication_demoted` and a per-group `demotion_reason` field;
  human output names the deciding diff source in the demotion note.
- Unified diffs: `fallow_output::unified_diff_events` is the one reader of
  unified diff text (`crates/output/src/unified_diff.rs`). It reads a file
  header only outside a hunk, decodes a quoted path of git, and removes the
  tab that git writes after a path with a space. `DiffIndex::from_unified_diff`
  and the change anchors of the walkthrough consume its events. One audit run
  reads its merge-base worktree diff at most one time (`RunChange::diff_index`
  in `crates/api/src/audit_run/outcome.rs`).
- Clone groups inside a base clone group: after the demotion, the same
  function marks an introduced clone group as inherited when each instance
  lies inside a different instance of one base clone group
  (`crates/api/src/audit_run/clone_containment.rs`). The check maps each head
  instance to its base path through the rename list and the diff renames,
  and to its base lines through the line changes of the diff
  (`DiffIndex::base_line_range`). A rewritten line at the start or end of an
  instance maps to the replaced base line next to the unchanged lines of the
  instance, so a block that turns several lines into fewer does not move the
  span outside its base instance. An edit inside one copy of a base clone
  splits the base group into smaller groups with new keys, and this rule
  keeps them inherited. The rule records no demotion, because the group
  matches a base group. A file that the diff adds has no base lines, so a
  pasted copy stays introduced. At least one instance must hold no added
  line: when every copy holds an added line, the change wrote the same text
  into each copy, and the group stays introduced.
- Audit clone-group gate: the `introduced-clone-group` rule decides how a
  clone group takes part in the verdict (`duplication_verdict_flags` in
  `crates/api/src/audit_run/outcome.rs`). Under `new-only`, only an
  introduced group counts; under `all`, every group in the changed files
  counts. `error` fails the verdict, `warn` (the default) gives `warn`, and
  `off` removes the group from the verdict but keeps it in the report. With
  `overrides`, the rule resolves for each instance file and the most severe
  result applies. So an override cannot exempt a clone group that has a copy
  outside the override. For generated code, use `duplicates.ignore` or
  `ignorePatterns`. The `duplicates.threshold` gate stays independent: it
  reads the same counted groups and also fails a run where the rule is
  `off`. The rule adds no suppression token; `code-duplication` suppresses a
  clone instance before attribution.
- Audit diff-risk rules: `crates/api/src/diff_risk/` computes `weakened-tests`,
  `added-suppression`, `test-only-branch`, `diverged-clone-sibling` and
  `added-dependency` inside `audit_run::run`, so the CLI, the MCP `audit` tool
  and `run_audit` share one result. The pass reads base text at the merge
  base through one `git cat-file --batch` process. It does not use the base
  snapshot, except `diverged-clone-sibling`, which reads the clone-group
  instance locations of the snapshot (`dupe_groups`, base paths, not
  remapped). A skipped base pass or `--gate all` turns that rule off. Rename
  sources are skipped, and a rename target reads its base text at the old
  path. `removed-tests` and `assertion-drop` report a file only when the total
  over all changed test files also dropped. An `added-suppression` finding
  honors only a suppression that names its rule, so a new blanket
  `fallow-ignore` comment cannot hide itself. The rule tokens are in
  `NON_CORE_KINDS`, so the dead-code stale-suppression pass ignores them.
  The brief `weakening[]` signals for tests, thresholds and suppressions are
  a projection of these findings (`crates/cli/src/audit_weakening.rs`). Do not
  add a second detector there. Two scans are local to the brief
  (`crates/cli/src/audit_security_scan.rs`), and both give
  `security-check-removed` signals: the removed-security-step scan of CI
  files, and the sink substitution scan of changed non-test source files
  (`crates/engine/src/sink_substitution.rs`). The second scan parses the base
  and the head text of each file and compares the non-literal sink sites per
  class (code execution, HTML). A class that loses one sink name and gains
  another gives `<old> sink replaced by <new>`. A removed sink whose name
  shows up in a new object key, computed member or alias gives
  `<old> sink moved into <form>`. Only a property sink (`innerHTML`, `srcdoc`,
  `dangerouslySetInnerHTML`) counts in an object key, and only with a
  non-literal value: `{ eval: 2 }` and `{ write: log }` are data, not a sink.
  The scan works per file, so a sink that only moves within the file gives
  nothing.
- Brief test reach: `test_reach` in the brief classifies each changed value
  export with `ModuleGraph::export_test_reach`
  (`crates/graph/src/graph/test_reach.rs`). The check reads the existing test
  reachability index and stores nothing new. Only value references count. The
  order is a covered reference from a test file (`direct`), any covered
  reference (`transitive`), a reference that a closed factory mock removed
  (`mocked-only`), then file reach (`none` or `none-file`). The mocked-only
  check replays the masks along each reference path: the mask step runs before
  the reachable step, and a CommonJS hop never masks. It runs only when
  masking is `active`. Past the profile cap, masking is `capped` and no row is
  `mocked-only`. The changed-export list comes from
  `changed_exports_for_changed_paths` in `crates/engine/src/module_graph.rs`,
  which parses each changed file once for the search gap and test reach
  passes. The CLI keeps only the exports whose declaration overlaps an added
  line before these passes run, so a wide diff does not classify untouched
  exports. The HEAD analyses read the diff, and the brief reuses that read. The
  `framework` label needs a name in `CONFIG_ONLY_EXPORT_NAMES`
  (`crates/core/src/plugins/mod.rs`), a plugin `used_exports` rule that names
  it for the file (recorded in the trace provenance), and a literal
  initializer. The coverage-gap note of an untested export uses the same
  mocked-only check. The `change` label of a row compares the export name
  with the export names of the file at the merge base of the diff
  (`export_names_at_base` in `crates/api/src/audit_run/base_files.rs`, one
  `git cat-file --batch` reader). A renamed file is read at its old path.
  When git cannot read the base file, the label falls back to the diff
  lines, and that fallback labels a one-line edit `new`. `e2e_runner` in the
  JSON and the human `none-file` reason come from the same
  `has_e2e_runner` fact.
- Audit dependency scope: a dependency-level finding (unused, type-only,
  test-only or misplaced dependency, unused catalog entry) is in audit scope
  only when the changeset touches the manifest or catalog file that declares
  it. The rule applies to the head run and to the base snapshot
  (`scope_dependency_findings` in `crates/api/src/audit_run/scope.rs`).
  `--changed-since` on the other commands keeps every dependency finding.
- Audit unprovable classes: a run does not report a dead-code class that it
  cannot prove. Today this is `unused_catalog_entries` while a workspace
  pattern directory is missing. The base snapshot is a checkout of committed
  files, so a gitignored workspace member (a cloned sub-repository) is missing
  there and present in the head. The base records these classes as
  `AuditKeySnapshot::unprovable_dead_code`, from
  `fallow_engine::unprovable::unprovable_dead_code_collections`. It reads the
  same config predicate that skips the detector
  (`has_missing_workspace_pattern_dirs`). `attribute` adds the head keys of those
  classes to the base key sets, so the verdict, the attribution counts and the
  `introduced` flags read them as inherited. An inherited base adds its
  classes to the set. A real new finding of such a class is also inherited in
  that run; the head run cannot tell it apart.
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
  The `GlobSet` matches all rules in one automaton, and its lookup time grows
  with the number of wildcards in the file. `CodeOwners::parse` rejects a
  pattern with more than `MAX_RULE_WILDCARDS` wildcards and a file with more
  than `MAX_FILE_WILDCARDS`. Large public CODEOWNERS files use fewer than 400.
  The `GlobSet` also builds an Aho-Corasick DFA over literal prefixes, and
  that build time grows with the square of the pattern length.
  `CodeOwners::parse` rejects a pattern longer than `MAX_RULE_BYTES`. Config
  globs have the same limit, `MAX_GLOB_BYTES` in `glob_validation.rs`.

- A stderr warning that repeats a `workspace_diagnostics[]` entry, or that
  describes the analysis inputs, uses the tracing target
  `fallow_types::ANALYSIS_DIAGNOSTIC_LOG_TARGET` (`fallow::analysis_diagnostic`).
  `build_tracing_filter` in `crates/cli/src/cli_startup.rs` turns that target
  off when the run is quiet (`--quiet`, `--ci` or `FALLOW_QUIET`) and
  `RUST_LOG` is not set. Use the target for a new warning of this type. Do not
  use it for a config-load warning, such as an unknown rule key, because a
  quiet run must still show those.
- `ignoreFindings` also covers parse degradations.
  `record_source_parse_degradations` drops the degradation of a covered file
  before the registry write and the stderr warning, so the envelope, the
  `parse-error` gate and the health note see the same set.
  `GraphConfidenceContext::for_run` skips a covered degraded module, and that
  check records the pattern as matched. The skip also applies to a reachable
  module or an entry point, because fixtures that a `dynamicallyLoaded` glob
  makes entry points are the common case. A covered file that is valid but
  fails to parse can hide the import that credits another module, with no
  caveat and no diagnostic.
- A dead-code baseline records the install state of the saving run in
  `analysis_identity.node_modules` through `BaselineAnalysisIdentity` in
  `crates/engine/src/baseline/install_state.rs`. The key is not part of
  `SemanticAnalysisIdentity`, so a difference never stops the comparison. On
  load, `check/baseline_notes.rs` prints the mismatch also under `--quiet` and
  stores the `baseline-install-state-mismatch` diagnostic on
  `LoadedBaselineStaleness::install_state`. `complete_check_execution` merges
  it into `workspace_diagnostics`. The programmatic API adds the same
  diagnostic in `apply_baseline`.

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
   duplication penalty counts each clone group with two or more instances in
   total and one or more in the group. The group counts only the lines of
   its own instances. Thus a clone that spans two groups lowers the score of
   each group, and the group `duplicated_lines` sum to the project value when
   the groups partition the files.

   The three scoped duplication lenses use different rules:
   - `health --group-by` measures the lines of each group, as above.
   - `dupes --group-by` assigns each clone group to the largest owner (the
     owner with the most instances), because it routes each finding to one
     owner. See `report/dupes_grouping.rs`.
   - `health --workspace` keeps only the clone instances inside the
     workspace and drops a clone group with fewer than two instances there.
     It measures copy-paste inside the workspace, not copy-paste across the
     workspace boundary.
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

The health envelope names the sections that the run produced in `sections`
(`HealthSection` in `crates/output/src/health_report.rs`).
`produced_sections` in `crates/engine/src/health/assembly.rs` builds the list
with the same gates that fill the report fields, so a token is in the list
exactly when the report carries that section. `complexity` means that
`findings` is the complexity list. Without it, `findings` is empty because
the run did not list the functions, and `summary.functions_above_threshold`
is the only count.

When a run does not list findings but `summary.functions_above_threshold` is
not zero, the complexity section gives that count and names `--complexity`.
It does not say that no function exceeds a threshold. One rule,
`fallow_api::ComplexityListing::count_unlisted`, decides this for both
renderers:

- A run whose `sections` does not contain `complexity` gives the count, also
  with a baseline. This applies to `--score`, `--hotspots`, `--file-scores`
  and `--targets`.
- A run whose `sections` contains `complexity` and lists no finding is clean.
  With a baseline, the baseline accepts every finding.
- An envelope without `sections` (from an older fallow, read with
  `report --from`) keeps the earlier rule: a run without `vital_signs`
  (score-only) gives the count, and another run gives the count only without
  a baseline.

The job summary renders only the complexity section, so it gives the count
for each run without `complexity`. Markdown gives the count only on a
score-only run. On `--hotspots`, `--file-scores` and `--targets`, Markdown
has no complexity section, as in the human output.

With a baseline, `summary.baseline_staleness.remaining_findings` is the
number of functions above a threshold that the baseline does not accept.
`load_health_baseline` in `crates/engine/src/health/baseline_io.rs` sets it
from the finding list after the baseline filter and before `--top`. It counts
functions, not baseline entries, because one function can match more than
one entry. The note (`ComplexityListing::not_listed_note`) then says "The
baseline accepts 2. 1 is new.". It asks for a `--complexity` run only when a
function is new. An envelope without the member keeps the earlier note: the
count includes the functions that the baseline accepts. `dead-code` and
`dupes` do not set the member.

## Saved markdown render

`fallow report --from <file> --format markdown` prints the same bytes as the
live `--format markdown` run that wrote the file. The live markdown has no
elapsed time, so no field is masked. `report::markdown::print_saved_markdown`
reads the typed sections back from the envelope and calls the section printers
of the live run in the same order:

- `dead-code` and `architecture`: the findings, the type-aware evidence and
  the config patterns that matched nothing. A grouped envelope
  (`dead-code-grouped` or `architecture-grouped`) is flattened to its flat
  kind, sorted into the order of the analysis, and grouped again with the
  saved `grouped_by` mode. `cli_report::GROUPED_SAVED_KINDS` lists the grouped
  kinds with their flat kind and current schema version.
- `dupes`: the duplication report. The live markdown of a grouped run is not
  grouped, so the saved render is not grouped.
- `health`: the full `HealthReport`, then the `## Health by <mode>` section
  from the envelope, then the type-aware evidence.
- the bare combined run: the `check`, `dupes` and `health` sections in that
  order.

The full health report reads back because the health output types in
`crates/output` and the action types in `crates/types` derive `Deserialize`.
A field that a live run fills from a string constant uses the
`static_str::StaticStr` alias and a `deserialize_with` function. serde borrows
a field spelled `&'static str` from the input, and the alias stops that borrow.

Three cases are refused with exit 2 and a reason on stderr:

- `audit`: the live audit markdown is the human report with markdown sections.
- `security` and `fix`: these commands print no markdown document.
- a combined envelope with type-aware evidence: the envelope keeps the evidence
  of the dead-code section only, but the live markdown also prints the evidence
  of the health section.

The combined envelope does not record `--group-by`, so the saved render of a
grouped combined run prints the dead-code section without groups and has no
`## Health by <mode>` section. The
`report_parity_tests` integration tests compare the two renders for each
supported kind.

## Saved-only mutation join

`fallow report --from` renders a saved envelope byte-identically to the
direct `--format` run, with two exceptions:

- The live PR comment of the bare combined run has its own layout.
  `report --from` renders the per-finding layout for it.
- `--mutation-report` and `--mutation-not-run` join the mutation evidence of
  a CI job to the render. No live command has this join, so these renders
  have no live equivalent. `SAVED_ONLY_JOINS` in `report_parity_tests.rs`
  lists the flags with their covering tests.

`cli_report::run_report` takes an `Option<MutationRequest>`: the input
(`MutationInput::Report(path)` or `MutationInput::NotRun(reason)`) and the
`--root` as the user wrote it. The inspect command in the section repeats
that root, not the absolute path of the runner. The order is:

1. `report_target`, then `validate_mutation_target`. A format other than the
   pr-comment, review, `github-summary`, `github-annotations` and
   `codeclimate` formats exits 2 before `--from` is read.
2. The envelope load and the kind detection, then `validate_mutation_kind`.
   A saved `fallow fix` envelope exits 2.
3. `validate_report_target` and `validate_saved_report_envelope`. An invalid
   `--from` file reports its own error first.
4. `join_mutation_evidence`: `MutationEvidence::load` or
   `MutationEvidence::not_run`, then `with_root_flag`, then
   `merge_gate_outcomes`. The merge adds only
   `gate_outcomes["mutation-min-score"]` to the envelope, so the status note,
   the summary and annotation gate lines and the decision gate rows read it.
   An input error exits 2 with `FALLOW_MUTATION_REPORT_INVALID: <message>`.
5. `MutationEvidence::codeclimate_issues`, called once. It builds the
   survivor issues, applies the shared diff filter
   (`diff_filter::filter_issues_from_env`), caps them at
   `MUTATION_MAX_INLINE`, and sets `inline_overflow` in the view. The filter
   runs before the cap, so a survivor outside the diff never takes an inline
   slot.
6. `print_saved_target` renders the target.

`crates/cli/src/report/mutation_evidence.rs` reads the report with the
`SavedReport` type of `mutation_inspect.rs`, so the two readers cannot
disagree. It checks the SHA-256 digest of each file under the resolved root.
A survivor in a changed or missing file is dropped and counted in
`stale_hidden`. A file that is not valid JSON, or a `mutation` report that
does not parse, is the `Unreadable` state with a stderr warning and exit 0,
so a truncated report never hides the analysis output.

Each surface has a `*_with_mutation` entry point. The earlier entry point
delegates to it with `None`, so the live callers and their output do not
change:

- PR comment (`print_pr_comment_with_mutation`): the survivors pass the
  summary filter and the path rebase on their own, and follow the analysis
  issues. `mutation_decision_row` goes after the command row and before the
  gate rows. `compose_trailing_section` puts the mutation section before the
  section of config patterns that matched nothing, joined by a blank line.
  With no mutation section, the trailing section is unchanged.
- Review (`print_review_envelope_with_mutation`): the analysis issues and
  the survivors are merged and sorted by path, line and fingerprint, so a
  survivor on the line of an analysis finding merges into one comment. The
  summary gets `mutation_review_summary_line` and the comment section.
- Annotations (`print_annotations_with_mutation`): the gate notice, the
  request notice, the run-state line, the analysis annotations, the survivor
  warnings, then the budget notice. The run-state line comes early, so the
  `head -n` cap of the Action keeps it.
- Job summary (`print_summary_with_mutation`): the `##` section goes after
  `render_summary` and before the config-pattern section.
- CodeClimate (`print_envelope_codeclimate_with_config`): the survivors follow
  the analysis issues, before the shared path rebase.

`crates/output/src/mutation_ci.rs` owns all the text and has no IO. A
`suggestion` fence never appears, because no fix intent in
`report/ci/suggestion.rs` matches `fallow/mutation-survived`.

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

## Quiet audit report

`fallow audit --quiet` prints the actionable view of the report in the
human, compact and markdown formats. `FALLOW_QUIET=1` sets quiet too.
`--ci` sets quiet, but its default format is SARIF, so only `--ci` with an
explicit `--format human`, `compact` or `markdown` gets the view. `audit_quiet.rs` builds the view after the verdict and
after the impact record, so the exit code and the impact data use the
complete result.

- The view keeps introduced and worsened findings, diff-risk findings, the
  configuration and analysis diagnostics, and the `insufficient` and
  `no_match` records of `health.thresholdOverrides`.
- The complexity section also prints when it holds only kept override
  records, so an `insufficient` record stays when every complexity finding
  is inherited.
- When no row is actionable, the human and markdown reports print one line,
  `No introduced or worsened findings.`. The compact report stays empty.
- The view drops inherited findings, the aggregate health metrics (vital
  signs, health score, file scores, hotspots, refactoring targets, trend,
  large functions and CSS analytics) and the `stale` and `active` override
  records.
- Without a base snapshot (`--gate all`), every finding is part of the
  gate, so the view keeps all findings and drops only the aggregate metrics
  and the routine override records.
- `DeadCodeAuditLedger::retain_introduced_results` filters the dead-code
  collections with the same exhaustive collection list as
  `annotate_results`. A finding without a ledger record stays.
- JSON, SARIF, CodeClimate, review, PR-comment and GitHub formats keep the
  complete report under `--quiet`. Their consumers read the inherited
  context and the JSON schema does not make these sections optional.
- The review brief and the walkthrough modes keep their own output
  contracts.

## Analysis selection

`fallow_config::plan_run` in `crates/config/src/config/run_plan.rs` is the one
rule that decides which of the dead-code, duplication and health analyses a
run executes. A surface builds a `RunRequest`, gives it to `plan_run` with the
`PlanSections` of its config (`plan_sections()` on `FallowConfig` and
`ResolvedConfig`), and reads the `RunPlan` that comes back.

A `RunRequest` holds a `RunCommand` and one `AnalysisRequest` for each
analysis:

- `RunCommand::Combined`: no command key applies. The bare `fallow` run,
  `fallow_api::run_combined`, the health score and the editor use it.
- `RunCommand::Audit`: the `audit.deadCode`, `audit.health` and `audit.dupes`
  keys apply.
- `Named`: the run names the analysis. The analysis runs.
- `Default`: the run does not name the analysis. The config decides.
- `Excluded`: the run leaves the analysis out.

The constructors map each surface to a request:

- `RunRequest::from_lists`: the `--only` and `--skip` lists of the bare run and
  of `fallow audit`. A non-empty `only` names each listed analysis and
  excludes the others. Otherwise `skip` excludes each listed analysis.
- `RunRequest::from_booleans`: the options of `fallow_api::run_combined`.
  `false` excludes the analysis. `true` names the analysis only when it is the
  one option that is `true`. Otherwise the config decides.
- `RunRequest::from_options`: the options of `fallow_api::run_audit`.
  `Some(true)` names, `Some(false)` excludes and `None` gives `Default`.
- `RunRequest::all_named`: the review brief, which runs every analysis.

`from_lists` and `from_booleans` have different rules for `Named` on purpose.
Do not merge them. The tests in `run_plan_tests.rs` pin both.

`planned_lists` in `crates/cli/src/combined/mod.rs` maps the `AnalysisKind`
lists to `PlannedAnalysis` lists. The `Architecture` mapping is asymmetric:

- In `--only`, `architecture` maps to the dead-code analysis, because the
  architecture findings are a part of it.
- In `--skip`, `architecture` maps to no analysis. The dead-code analysis
  still runs, and `resolve_architecture_selection` removes only its
  architecture findings. That function reads no config key.

`plan_run` applies this rule to each analysis:

1. `Named` gives `Run`.
2. `Excluded` gives `Skipped(SkipReason::NotRequested)`.
3. For `Default` with `RunCommand::Audit`, the `audit` key decides first.
   `false` gives `Skipped(SkipReason::Config(key))`. `true` gives `Run`, also
   when `duplicates.enabled` is `false`.
4. For the duplication analysis, `duplicates.enabled: false` gives
   `Skipped(SkipReason::Config(ConfigKey::DuplicatesEnabled))`.
5. Otherwise the analysis runs.

`RunRequest::reads_config` tells whether the plan depends on the config. The
bare run (`load_run_plan` in `crates/cli/src/cli_production.rs`) and the API
runtime load the config only when it does.

`AnalysisSelection::off_by` tells which key turned an analysis off. The
`--dupes-baseline` flag uses it:

- When `duplicates.enabled` alone turned duplication off, the flag gives a
  note and the exit code does not change.
- When `audit.dupes: false` turned duplication off, the flag is an error with
  exit code 2.

The health score and the editor analysis call
`DuplicatesConfig::runs_by_default`, which is the plan of a `Combined` request
with every analysis `Default`. The flags of a run do not change that answer:
`--only dupes,health` prints the clone groups and keeps the score without a
duplication penalty.

An audit and a bare `fallow` run write an Impact record only for a full
selection. Both commands use one rule, `AuditAnalysisSelection::records_impact`:

- `is_full_without_dupes` must be true: the dead-code and health analyses run,
  with every dead-code finding.
- The duplication analysis must also run, or `duplicates_off_in_config` must
  be true. That field holds `!DuplicatesConfig::runs_by_default()`, so it does
  not depend on the flags.

With `duplicates.enabled: false`, `--skip dupes` and `--only dead-code,health`
narrow nothing, so both commands write a record. When `--only` and `--skip`
leave out duplication, the project has duplication on, and Impact writes no
record in either command. `audit.dupes: false` writes no audit record, because
the bare run ignores that key and runs duplication.

A record fills the stored clone frontier only when its run executed the
duplication analysis. With duplication off, a run with
`--only dead-code,dupes,health` executes it, and so does a run before the
config change. `AttributionInput::clones` is `None` when the run did not
execute duplication. Impact then credits no clone group as resolved and keeps
the clone frontier. A later run with duplication compares with that frontier:
a clone that is still there gives no credit, and a removed clone gives one.
A run that executed duplication and found no clone gives `Some` with an empty
list, so it credits each stored clone group that touches the changed files.

Do not read a selection key outside the config crate. The tests in
`crates/cli/src/selection_key_guard_tests.rs` fail when a production file
reads `duplicates.enabled`, `audit.deadCode`, `audit.health` or `audit.dupes`:

- A text needle finds `.duplicates.enabled` in every Rust file under `crates/`.
- A `syn` visitor finds each read of a selection key outside test code: a
  field read on a `duplicates` or `audit` field access, on a binding of a
  `DuplicatesConfig` or `AuditConfig` type, or on an untyped `let` of such a
  field access. It also finds a destructure that binds a key, and the same
  token sequence inside a macro body.
- `SELECTION_KEY_OWNERS` names each file that may read a key, with the fact it
  owns. A reverse check fails when a named file no longer reads a key.
- The visitor cannot see the return type of a method that is bound to an
  untyped `let`.

## Parse cache of the audit worktrees

`fallow audit` analyzes two kinds of temporary worktree: the base snapshot,
and the staged index of `--staged`. Each worktree root has another path than
the checkout, so the config of the root resolves another cache directory.
`crates/cli/src/audit_worktree_cache.rs` gives each worktree root the cache
directory `<worktree root>/.fallow`, also when `FALLOW_CACHE_DIR` or
`cache.dir` names another directory. `WorktreeParseCache::lend` copies the
parse cache (`cache.bin`) of the checkout into that directory when the
directory has no parse cache yet.

- The parse cache keys each entry by the root-relative path and the content
  hash. A worktree file whose content differs from the cached entry is parsed
  again, so the checkout cache cannot give stale results. On Unix, a file in
  a new worktree never matches the size, mtime and ctime of a cached entry. On
  Windows, the cache does not record a ctime and never trusts metadata alone.
  On both, the run hashes the content of each file.
- The module graph cache records the project root, and the duplication token
  cache records absolute paths. A worktree writes these caches only in its
  own directory. Otherwise a shared `FALLOW_CACHE_DIR` gets the module graph
  cache of a worktree root, and the next run of the checkout rejects it with
  `root-mismatch`.
- The staged run writes its updated parse cache back to the checkout after
  the run (`WriteBack::Yes`), so the next staged run of the same content
  parses no file. It skips the write when another run replaced the parse
  cache of the checkout during the staged run. The base pass does not write
  back (`WriteBack::No`): its parse cache holds the base content, and the
  reused base worktree keeps it.
- The cost of the write-back is a reparse, never a wrong result. The entries
  carry the size and times of the worktree files, so the next run of the
  checkout hashes each file once and stores its own times again. The staged
  worktree has no untracked file of the checkout, so the save of the staged
  run drops the entries of those files.

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
- Base-snapshot payload files: each audit base pass that misses the cache
  writes `<cache>/cache/audit-base-v<N>/<key-hash>.bin`, and the key folds in
  the base SHA and the changed files. After each save, audit removes the
  files that are older than the resolved threshold (measured from the last
  write or cache hit, because a hit refreshes the file mtime) and keeps only
  the newest `MAX_AUDIT_BASE_SNAPSHOT_FILES` (32) files. The cap applies
  also when the threshold is `0`. Prune reports these files with
  `pass: "snapshot"` and reasons `aged-out`, `over-capacity`, `fresh` or
  `age-gc-disabled`. It resolves the cache directory from
  `FALLOW_CACHE_DIR`, then `cache.dir`, then `<root>/.fallow`
  (`crates/cli/src/audit_cache.rs`).

Shared invariants (`crates/cli/src/base_worktree.rs`):

- Both prune modes and the per-audit sweep share one decision code path
  (`sweep_reusable_caches_with_report`), so prune can never drift from what
  audits actually reclaim.
- The per-audit sweep scans the temp dir for legacy and cross-repo entries
  at most once per hour (`FULL_SWEEP_INTERVAL` in `crates/cli/src/base_worktree_scan.rs`). The mtime of the
  `fallow-audit-cache-sweep-<euid>.stamp` marker in the temp dir records the
  last scan. Audits inside the interval sweep only the requested root's own
  entry and the git-registered legacy entries (`SweepScope::OwnedOnly`), so
  their cost does not grow with the number of entries in the temp dir. A
  missing, old, future-dated or untrusted marker gives a full scan. Prune
  always scans (`SweepScope::All`) and does not read or write the marker.
- `--dry-run` performs zero filesystem mutation: no cache removal, no
  `.lock` sidecar creation (acquiring a lock would create one), no
  `.last-used` grace seeding, and no git worktree deregistration. The legacy
  registered-cache pass is still enumerated read-only with the same
  `legacy-registered` / `legacy-deregistered` split an apply run reports.
- On Unix the sweep unlinks the `.lock` sidecar of a reclaimed entry while it
  holds the flock, and reports a bare lock sidecar as `orphaned-lock`. This is
  race-safe because `ReusableWorktreeLock::try_acquire` checks after the flock
  that its fd is still the inode at the path, and opens the path again when
  it is not. Without that check, a process that opened the old inode before
  the unlink could hold a flock on it while another process locks the new
  file. Windows keeps the `.lock` sidecar and reports `lock-only`.
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

## Trace targets

- `fallow trace` has three targets. `dispatch_trace_command`
  (`crates/cli/src/dispatch.rs`) runs `trace_path::run_trace_path` for `--path`
  and `trace_chain::run_trace` for a `FILE:SYMBOL`. `--dependency` goes to
  `dispatch_trace_dependency` and `trace_dependency::run_trace_dependency`.
- `fallow trace --dependency` and `fallow dead-code --trace-dependency` build
  the base `DependencyTrace` with one engine function,
  `fallow_engine::trace::build_dependency_trace`. The programmatic API uses
  it too. Do not add a fourth copy of the source, credit and manifest steps.
- The dependency runner honors `--production`, `--no-production`, the
  `FALLOW_PRODUCTION*` variables and `--workspace` as `fallow dead-code` does:
  the resolver decides the production mode of the `TraceDependency` run, and
  the workspace scope is validated with `check::resolve_workspace_scope`. The
  scope does not narrow the trace. The `--path`, symbol and `trace-error`
  runners read the production mode from the config only.
- The usage walk (`crates/engine/src/trace_usage_impl.rs`) reads
  `ModuleInfo.import_binding_references`, `imported_call_sites`, imports,
  re-exports, dynamic imports and `require` calls. It needs modules that
  `release_resolution_payload` did not release, so
  `trace_dependency_with_session` calls
  `analyze_dead_code_with_shared_artifacts(true, true)`.
- Only `json` and `human` are supported. Human output goes to stdout through
  `outln!`; `fallow dead-code --trace-dependency` keeps its stderr block.
- A wrapper is an exported top-level declarator whose initializer is a
  runtime binding of the package (`alias` shape) or a call on one (`call`
  shape). The walk follows one hop from each export name of the wrapper:
  named, default and namespace imports of the wrapper module. It also counts
  the uses of the wrapper in its own module (`through` references from
  extraction) and each project re-export of the wrapper as a `re_export` site
  with `via`. It does not follow the consumers of that re-export.
- A call wrapper returns a value. A call of a member of that value
  (`store.dispatch()` for `export const store = configureStore()`) is a
  `non_call_reference`, not a call of the package export. The call that
  defines a call wrapper counts in `call_site_count`.
- A wrapper of a whole namespace (`export const R = RR`) takes the specifier
  of each consumer site from the first member (`R.useSelector()` gives
  `useSelector`).

## CLI reference contract

`fallow schema` prints the capability manifest for agents. `fallow schema
--cli-reference` prints the CLI reference: one JSON document from which a
documentation generator writes a page per command, the option tables, the
exit code table, the environment variable table and a page per issue type.
`crates/cli/src/schema/cli_reference.rs` builds it. The generator commits the
output as `docs/cli-reference.json`, and `npm run generate:contracts:check`
fails when the file is stale.

The document has these top-level fields:

- `schema_version`: the version of the document shape. A change that removes
  or renames a field raises it. A new field does not.
- `version`, `name`, `about`, `usage`: the binary and its root usage line.
- `root_command`: the positionals and the options of bare `fallow` that are
  not global.
- `global_options[]`: the options that every command accepts. A command does
  not repeat them in its own `options[]`.
- `commands[]`: the top-level commands in the order and with the
  `help_group` heading of the root `--help`, then the hidden commands with a
  null `help_group`. Each command has `name`, `path` (for example
  `coverage upload-inventory`), `aliases[]`, `about`, `long_about`,
  `hidden`, `usage`, `always_json`, `exit_codes[]`, `positionals[]`,
  `options[]` and `subcommands[]` at each depth. The automatic `help`
  subcommand is left out.
- `exit_codes[]`: the codes that every command shares, each with `code`,
  `name` and `description`. A command that gives other codes, or gives a
  code another meaning, lists them in its own `exit_codes[]`.
- `environment_variables[]`: the registry of `fallow schema`, as `name` and
  `description` pairs.
- `output_formats[]`: the values of `--format` with their aliases and help.
- `issue_types[]`: the issue type rows of `fallow schema`, each with an
  `explanation` object that holds the text of
  `fallow explain <id> --format json`. `example` and `how_to_fix` are null
  for an issue type that has no guide of its own. `example_files` and
  `example_base_files` hold the visible files of the example project in
  `crates/api/rule-examples/<id>/`, and are empty for an issue type that
  has none.

Each option has `id`, `long`, `short`, `aliases[]` (each with `visible`),
`value_name`, `action`, `takes_value`, `multiple`, `possible_values[]` (each
with `help`, `aliases` and `hidden`), `default`, `env`, `help`, `long_help`,
`hidden`, `hidden_in_short_help`, `global`, `help_heading`, `required`,
`conflicts_with[]`, `negates` and `negated_by`. `env` is null: clap binds no
environment variable, and the code reads each variable itself.
`negates` names the `--x` flag of a `--no-x` flag, and `negated_by` names the
`--no-x` flag of a `--x` flag, when the command has both. `always_json` is
true when the command writes JSON for each `--format`, false when it follows
`--format`, and null when the command needs arguments to parse.

The help groups come from `TOP_LEVEL_AFTER_LONG_HELP`. The exit codes of a
command come from `COMMAND_EXIT_CODES`, which names the constants of the
command module. A test walks the clap tree and the issue type registry, so a
command, an option or an issue type that the document misses fails the build.
Another test compares the tables of `docs/environment-variables.md` with the
environment variable registry.

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
