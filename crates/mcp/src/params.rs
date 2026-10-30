//! MCP tool parameter structs.
//!
//! Doc comments feed both rustdoc and the published JSON Schema.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Deserialize;

/// Privacy mode for author emails emitted by `--ownership`.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EmailModeParam {
    Raw,
    Handle,
    Anonymized,
    Hash,
}

impl EmailModeParam {
    pub const fn as_cli(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Handle => "handle",
            Self::Anonymized => "anonymized",
            Self::Hash => "hash",
        }
    }
}

/// How a health baseline matches findings, mirroring the CLI's
/// `--baseline-mode` flag.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BaselineModeParam {
    Count,
    Identity,
}

impl BaselineModeParam {
    pub const fn as_cli(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Identity => "identity",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TypeAwareRequireParam {
    BestEffort,
    Complete,
}

impl TypeAwareRequireParam {
    pub const fn as_cli(self) -> &'static str {
        match self {
            Self::BestEffort => "best-effort",
            Self::Complete => "complete",
        }
    }
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct AnalyzeParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Production mode: exclude test, story, and dev files, only start/build
    /// scripts, and report type-only dependencies.
    pub production: Option<bool>,

    /// Ignore the per-package refs of `workspaces.changedSince` for this call,
    /// so every workspace package is analyzed in full scope. Use it to judge
    /// or save a whole-project baseline.
    pub no_package_baselines: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Issue types to include, for example `["unused-exports", "unused-files"]`;
    /// empty or omitted means all registered issue types.
    pub issue_types: Option<Vec<String>>,

    /// Analyze only architecture boundary violations.
    pub boundary_violations: Option<bool>,

    /// Compare results against a saved baseline file. Only new issues (not in
    /// the baseline) are reported. The response carries `baseline_staleness`:
    /// read `gate_trips` to see whether the baseline has entries that match
    /// nothing any more (re-save it with `save_baseline`), and read
    /// `change_scoped` first, because a run narrowed to part of the project can
    /// report `matched_entries: 0` on a perfectly healthy baseline.
    /// `scope_reasons` names the channels that narrowed it.
    /// `unrecognised_format: true` means the file is not this command's baseline.
    /// It suppressed nothing, so use another path and do not re-save.
    /// `saved_by` then names the command that wrote it, when fallow knows it.
    pub baseline: Option<String>,

    /// Save current results as a baseline file for future comparisons. The path
    /// must resolve inside the project root, its Git work tree, the CI workspace or a temp directory.
    /// A relative path resolves against the working directory of the MCP
    /// server, not against `root`, so pass an absolute path.
    pub save_baseline: Option<String>,

    /// Fail if the issue count increased beyond `tolerance` compared to a
    /// regression baseline.
    pub fail_on_regression: Option<bool>,

    /// Allowed issue count increase before a regression is flagged. Defaults to "0".
    pub tolerance: Option<String>,

    /// Path to the regression baseline file.
    pub regression_baseline: Option<String>,

    /// Save the current issue counts as a regression baseline file at this
    /// path. An empty or absent value writes nothing: this tool cannot write the
    /// counts into the fallow config. The path must resolve inside the project
    /// root, its Git work tree, the CI workspace or a temp directory.
    /// A relative path resolves against the working directory of the MCP
    /// server, not against `root`, so pass an absolute path.
    pub save_regression_baseline: Option<String>,

    /// Group findings by CODEOWNERS ownership, directory, workspace package, or
    /// GitLab CODEOWNERS section. Values: "owner", "directory", "package", "section".
    pub group_by: Option<String>,

    /// Scope analysis to these project-relative files instead of the whole project.
    pub file: Option<Vec<String>>,

    /// Report only these `finding_id` values. The response adds
    /// `finding_id_query`; a missing id is resolved only when `conclusive` is true.
    pub finding_ids: Option<Vec<String>>,

    /// Report unused exports in entry files instead of auto-marking them as used.
    pub include_entry_exports: Option<bool>,

    /// Keep the unused exports, types, class members and enum members of
    /// unused files. By default the report hides them and counts them in
    /// `cascade_hidden`. Passed through to the CLI's `--show-cascade` flag.
    pub show_cascade: Option<bool>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Refine project-wide public API findings with TypeScript symbol identity.
    /// `false` keeps the run syntactic, also when the config turns type-aware analysis on. `type_aware_projects` or `type_aware_require` turns it on and wins over `false`.
    pub type_aware: Option<bool>,

    /// Explicit tsconfig paths for type-aware analysis.
    pub type_aware_projects: Option<Vec<String>>,

    /// Whether incomplete semantic evidence is advisory or gating.
    pub type_aware_require: Option<TypeAwareRequireParam>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct CombinedParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Production mode: exclude test, story, and dev files, only start/build
    /// scripts, and report type-only dependencies.
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Git ref to compare against when limiting all combined sections to
    /// changed files.
    pub changed_since: Option<String>,

    /// Report unused exports in entry files instead of auto-marking them as used.
    pub include_entry_exports: Option<bool>,

    /// Keep the unused exports, types, class members and enum members of
    /// unused files. By default the report hides them and counts them in
    /// `cascade_hidden`. Passed through to the CLI's `--show-cascade` flag.
    pub show_cascade: Option<bool>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Duplicate-detection mode for the dupes section: "strict", "mild",
    /// "weak", or "semantic". Defaults to the project config, or "mild" if unset.
    pub dupes_mode: Option<String>,

    /// Enable function-scoped near-miss clone detection in the dupes section.
    pub dupes_near: Option<bool>,

    /// Override the minimum token count for clones in the dupes section.
    pub dupes_min_tokens: Option<u32>,

    /// Override the minimum line count for clones in the dupes section.
    pub dupes_min_lines: Option<u32>,

    /// Override the minimum clone occurrences in the dupes section (must be >= 2).
    #[schemars(range(min = 2))]
    pub dupes_min_occurrences: Option<u32>,

    /// Fail if duplication exceeds this percentage in the dupes section (0 = no limit).
    pub dupes_threshold: Option<f64>,

    /// Only report cross-directory duplicates in the dupes section.
    pub dupes_skip_local: Option<bool>,

    /// Omit clone instances whose path is a symlink or lies under a symlinked
    /// directory in the dupes section. Defaults to the project config; set
    /// `false` to report them even when the config sets `ignoreSymlinks`.
    pub dupes_ignore_symlinks: Option<bool>,

    /// Enable cross-language duplicate detection in the dupes section.
    pub dupes_cross_language: Option<bool>,

    /// Exclude import declarations from clone detection. Defaults to the
    /// project config.
    pub dupes_ignore_imports: Option<bool>,

    /// Include only complexity findings in the health section.
    pub complexity: Option<bool>,

    /// Include per-file scores in the health section.
    pub file_scores: Option<bool>,

    /// Include project health score in the health section.
    pub score: Option<bool>,

    /// Include refactoring targets in the health section.
    pub targets: Option<bool>,

    /// Include hotspots in the health section.
    pub hotspots: Option<bool>,

    /// Maximum cyclomatic complexity threshold.
    pub max_cyclomatic: Option<u16>,

    /// Maximum cognitive complexity threshold.
    pub max_cognitive: Option<u16>,

    /// Maximum CRAP score threshold.
    pub max_crap: Option<f64>,

    /// Maximum physical lines per file (default 0, off). A longer file is
    /// reported as a complexity finding with `exceeded: "file_lines"`.
    pub max_file_lines: Option<u32>,

    /// Istanbul or raw V8 coverage file or directory used by the health section.
    /// When omitted, `FALLOW_COVERAGE` and then `health.coverage` apply.
    pub coverage: Option<String>,

    /// Absolute prefix to strip from Istanbul coverage paths.
    /// When omitted, `FALLOW_COVERAGE_ROOT` and then `health.coverageRoot`
    /// apply.
    pub coverage_root: Option<String>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct ArchitectureParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Production mode: exclude test, story, and dev files, only start/build
    /// scripts, and report type-only dependencies.
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Report only findings in files changed since this git ref.
    pub changed_since: Option<String>,

    /// Only import cycles.
    pub cycles: Option<bool>,

    /// Only boundary violations, coverage and forbidden calls.
    pub boundaries: Option<bool>,

    /// Only rule-pack policy violations.
    pub policy: Option<bool>,

    /// Saved dead-code baseline file; only new findings are reported.
    pub baseline: Option<String>,

    /// "owner", "directory", "package", or "section".
    pub group_by: Option<String>,

    /// Project-relative files to scope the report to.
    pub file: Option<Vec<String>>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CheckChangedParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Git ref to compare against (e.g. "main", "HEAD~5"). Only files changed
    /// since this ref are analyzed. Required.
    pub since: String,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Production mode: exclude test, story, and dev files, only start/build
    /// scripts, and report type-only dependencies.
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Compare results against a saved baseline file. Only new issues (not in
    /// the baseline) are reported. The response carries `baseline_staleness`:
    /// read `gate_trips` to see whether the baseline has entries that match
    /// nothing any more (re-save it with `save_baseline`), and read
    /// `change_scoped` first, because a run narrowed to part of the project can
    /// report `matched_entries: 0` on a perfectly healthy baseline.
    /// `scope_reasons` names the channels that narrowed it.
    /// `unrecognised_format: true` means the file is not this command's baseline.
    /// It suppressed nothing, so use another path and do not re-save.
    /// `saved_by` then names the command that wrote it, when fallow knows it.
    pub baseline: Option<String>,

    /// Save current results as a baseline file for future comparisons. The path
    /// must resolve inside the project root, its Git work tree, the CI workspace or a temp directory.
    /// A relative path resolves against the working directory of the MCP
    /// server, not against `root`, so pass an absolute path.
    pub save_baseline: Option<String>,

    /// Fail if the issue count increased beyond `tolerance` compared to a
    /// regression baseline.
    pub fail_on_regression: Option<bool>,

    /// Allowed issue count increase before a regression is flagged. Defaults to "0".
    pub tolerance: Option<String>,

    /// Path to the regression baseline file.
    pub regression_baseline: Option<String>,

    /// Save the current issue counts as a regression baseline file at this
    /// path. An empty or absent value writes nothing: this tool cannot write the
    /// counts into the fallow config. The path must resolve inside the project
    /// root, its Git work tree, the CI workspace or a temp directory.
    /// A relative path resolves against the working directory of the MCP
    /// server, not against `root`, so pass an absolute path.
    pub save_regression_baseline: Option<String>,

    /// Report unused exports in entry files instead of auto-marking them as used.
    pub include_entry_exports: Option<bool>,

    /// Keep the unused exports, types, class members and enum members of
    /// unused files. By default the report hides them and counts them in
    /// `cascade_hidden`. Passed through to the CLI's `--show-cascade` flag.
    pub show_cascade: Option<bool>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,
}

/// Parameters for the `security_candidates` tool.
///
/// Security analysis can exceed the default MCP subprocess timeout on large
/// repos. Raise `FALLOW_TIMEOUT_SECS` in the server environment when needed.
#[derive(Default, Deserialize, JsonSchema)]
pub struct SecurityCandidatesParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Workspace packages to scope candidates to; mutually exclusive with `changed_workspaces`.
    pub workspace: Option<String>,

    /// Git ref to compare against when limiting candidates to changed files.
    pub changed_since: Option<String>,

    /// Scope candidates to just-edited files for the agent edit loop. Each path
    /// is passed to `fallow security --file` and matches finding anchors or
    /// trace hops.
    pub paths: Option<Vec<String>>,

    /// Scope candidates to workspaces touched since this git ref. Mutually
    /// exclusive with `workspace`.
    pub changed_workspaces: Option<String>,

    /// Include the attack-surface inventory with defensive-boundary verification
    /// prompts in the JSON response.
    pub surface: Option<bool>,

    /// Optional security regression gate. Valid values: "new" or
    /// "newly-reachable". The "newly-reachable" gate requires `changed_since`.
    pub gate: Option<String>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

/// Parameters for advisory semantic similar-code discovery.
///
/// The exact first-party model runs locally. This tool never performs model
/// setup and returns an actionable structured error when explicit setup is
/// still required.
#[derive(Default, Deserialize, JsonSchema)]
pub struct FindSimilarCodeParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Workspace packages to scope candidates to; mutually exclusive with `changed_workspaces`.
    pub workspace: Option<String>,

    /// Git ref used to report candidate pairs touching changed files.
    pub changed_since: Option<String>,

    /// Scope candidate reporting to workspaces touched since this git ref.
    pub changed_workspaces: Option<String>,

    /// Report candidate pairs touching one of these project-relative files.
    pub paths: Option<Vec<String>>,

    /// Minimum cosine similarity from zero through one. This is not a
    /// probability or verification verdict.
    pub threshold: Option<f64>,

    /// Minimum extracted source lines per function.
    pub min_lines: Option<usize>,

    /// Cap returned candidates after bounded full-corpus comparison.
    pub top: Option<usize>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

/// Parameters for reproducing one similar-code candidate and collecting its
/// bounded source-grounded evidence packet.
#[derive(Deserialize, JsonSchema)]
pub struct InspectSimilarCodeParams {
    /// Snapshot-stable candidate identity returned by `find_similar_code`.
    pub candidate_id: String,

    /// Bounded exact-candidate snapshot object selected from the original
    /// discovery response. Pass generation, candidate, completion, and
    /// diagnostics unchanged; inspect validates current source hashes before
    /// enrichment. Read `fallow://schema/similar-code-snapshot` for the full
    /// object shape; a snapshot that does not match it is refused with
    /// `FALLOW_MCP_INVALID_CANDIDATE_SNAPSHOT`.
    ///
    /// Deserialized as an untyped `Value` because the shape check lives in the
    /// handler, but published with the `object` constraint it always had:
    /// unconstrained, a client-side validator would accept a string or a
    /// number here and only the server would say no.
    #[schemars(with = "std::collections::BTreeMap<String, serde_json::Value>")]
    pub snapshot: serde_json::Value,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

/// Parameters for the static mutation plan. The plan runs no project code.
#[derive(Default, Deserialize, JsonSchema)]
pub struct MutationPlanParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Base ref for the default changed-code scope. Omitted: the base is
    /// detected as for `audit`.
    pub changed_since: Option<String>,

    /// Plan every source file instead of the changed code.
    pub all: Option<bool>,

    /// Plan only these files or directories, relative to the root.
    pub paths: Option<Vec<String>>,

    /// Maximum number of mutants to plan; the rest are `pending`. A run fills
    /// the limit with covered mutants, so it can also run a `pending` one.
    pub max_mutants: Option<usize>,

    /// Kebab-case operator names, for example `relational-operator`.
    /// Omitted: the `mutation.operators` config key, else every operator.
    pub operators: Option<Vec<String>>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,
}

/// Parameters for one mutant of a saved mutation report.
#[derive(Deserialize, JsonSchema)]
pub struct MutationInspectParams {
    /// The `mutant_id` from a mutation plan or report.
    #[schemars(length(min = 1))]
    pub mutant_id: String,

    /// Path to a saved `fallow mutation` report or `mutation_plan` output.
    /// A relative path resolves against `root`. Without `root`, it resolves
    /// against the working directory of the MCP server. Omitted: the report
    /// that the last `fallow mutation` run saved in the Fallow cache of the
    /// project.
    #[schemars(length(min = 1))]
    pub report: Option<String>,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file. Used only without `report`, to find the
    /// cache directory that holds the report of the last run.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Used only
    /// without `report`, as for `config`. Defaults to false and never grants
    /// process-global trust.
    pub allow_remote_extends: Option<bool>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct FindDupesParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Ignore the per-package refs of `workspaces.changedSince` for this call,
    /// so every workspace package is analyzed in full scope. Use it to judge
    /// or save a whole-project baseline.
    pub no_package_baselines: Option<bool>,

    /// Detection mode: "strict", "mild", "weak", or "semantic". Defaults to
    /// the project config, or "mild" if unset.
    pub mode: Option<String>,

    /// Detect function-scoped near-miss clones in addition to exact clones.
    pub near: Option<bool>,

    /// Minimum token count for a clone. Defaults to the project config, or 50 if unset.
    pub min_tokens: Option<u32>,

    /// Minimum line count for a clone. Defaults to the project config, or 5 if unset.
    pub min_lines: Option<u32>,

    /// Minimum number of occurrences before a clone group is reported. Raise
    /// to focus on widespread copy-paste and skip pair-only clones. Defaults
    /// to the project config, or 2 if unset.
    #[schemars(range(min = 2))]
    pub min_occurrences: Option<u32>,

    /// Fail if duplication exceeds this percentage (0 = no limit). Defaults
    /// to the project config, or 0 if unset.
    pub threshold: Option<f64>,

    /// Only report cross-directory duplicates.
    pub skip_local: Option<bool>,

    /// Omit clone instances whose path is a symlink or lies under a symlinked
    /// directory. Defaults to the project config; set `false` to report them
    /// even when the config sets `ignoreSymlinks`.
    pub ignore_symlinks: Option<bool>,

    /// Enable cross-language detection (strips TypeScript type annotations for TS/JS matching).
    pub cross_language: Option<bool>,

    /// Exclude import declarations from clone detection. Defaults to `true`
    /// (sorted import blocks are a formatting artifact, not copy-paste); set
    /// `false` to count them again.
    pub ignore_imports: Option<bool>,

    /// Show a per-pattern breakdown for default duplicate ignores. `dupes`
    /// prints only that breakdown; the built-in discovery ignores are reported
    /// by `check`, `dead-code`, `audit` and the default run.
    /// Human-format only (human/markdown CLI output); MCP JSON responses suppress the note.
    pub explain_skipped: Option<bool>,

    /// Show only the N highest-ranked clone groups. `clone_families[]` is
    /// rebuilt from the groups that survive, so it narrows too. `stats` keeps
    /// describing the whole corpus; read `clone_groups_shown` /
    /// `clone_groups_omitted` and `clone_families_shown` /
    /// `clone_families_omitted` for the split.
    pub top: Option<usize>,

    /// Include the verbatim source text on each clone instance. Defaults to
    /// false: `file` plus the line and column range address the same code, and
    /// the text is most of the response. Set true only when you need the
    /// source without reading the files.
    pub include_fragments: Option<bool>,

    /// Compare results against a saved baseline file. Only new clone groups
    /// (not in the baseline) are reported. The response carries
    /// `baseline_staleness`: read `gate_trips` to see whether the baseline has
    /// entries that match nothing any more (re-save it with `save_baseline`),
    /// and read `change_scoped` first, because a run narrowed to part of the
    /// project can report `matched_entries: 0` on a healthy baseline.
    /// `scope_reasons` names the channels that narrowed it.
    /// `unrecognised_format: true` means the file is not this command's baseline.
    /// It suppressed nothing, so use another path and do not re-save.
    /// `saved_by` then names the command that wrote it, when fallow knows it.
    pub baseline: Option<String>,

    /// Save current results as a baseline file for future comparisons. The path
    /// must resolve inside the project root, its Git work tree, the CI workspace or a temp directory.
    /// A relative path resolves against the working directory of the MCP
    /// server, not against `root`, so pass an absolute path.
    pub save_baseline: Option<String>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Git ref to compare against when limiting duplication to changed files.
    pub changed_since: Option<String>,

    /// Group findings by CODEOWNERS ownership, directory, workspace package, or
    /// GitLab CODEOWNERS section. Values: "owner", "directory", "package", "section".
    pub group_by: Option<String>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct FixParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Production mode: exclude test, story, and dev files, only start/build
    /// scripts, and report type-only dependencies.
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Refuse to create a new fallow config file when none exists. Use this
    /// from pre-commit hooks and CI bots where silently materializing a new
    /// top-level config file would surprise the user.
    pub no_create_config: Option<bool>,

    /// Also fix the unused exports, types, class members and enum members
    /// of unused files. By default `fix` skips them, as the report hides
    /// them. Passed through to the CLI's `--show-cascade` flag.
    pub show_cascade: Option<bool>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct ProjectInfoParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Include entry-point patterns in the response.
    pub entry_points: Option<bool>,

    /// Include discovered source files in the response.
    pub files: Option<bool>,

    /// Include active framework plugins in the response.
    pub plugins: Option<bool>,

    /// Include discovered architecture boundary zones in the response.
    pub boundaries: Option<bool>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,
}

/// Parameters for the `inspect_target` tool.
///
/// The tool composes several existing read-only analyses into one evidence
/// bundle. Large repositories can exceed the default MCP subprocess timeout;
/// raise `FALLOW_TIMEOUT_SECS` in the server environment when needed.
#[derive(Deserialize, JsonSchema)]
pub struct InspectTargetParams {
    /// The file or exported symbol to inspect. See the tool description for
    /// the two addressing shapes.
    pub target: InspectTarget,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Production mode: exclude test, story, and dev files, only start/build
    /// scripts, and report type-only dependencies. Forwarded only to child
    /// analyses that support it: trace, dead-code, and health.
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Include project-wide TypeScript semantic evidence for symbol targets.
    /// `false` keeps the run syntactic, also when the config turns type-aware analysis on. `type_aware_projects` or `type_aware_require` turns it on and wins over `false`.
    pub type_aware: Option<bool>,

    /// Explicit tsconfig paths for type-aware analysis.
    pub type_aware_projects: Option<Vec<String>>,

    /// Whether incomplete semantic evidence is advisory or gating.
    pub type_aware_require: Option<TypeAwareRequireParam>,

    /// OPT-IN (default off): attach target-level git churn evidence. Missing
    /// git history is returned as an explicit unavailable evidence section.
    pub include_churn: Option<bool>,

    /// OPT-IN (default off): also attach the best-effort symbol-level call chain
    /// (`fallow trace`) as the `symbol_chain` evidence section. Only
    /// meaningful for a SYMBOL target. Best-effort, syntactic (ADR-001), and
    /// EXPLICITLY OFF the ranked path: resolved-vs-unresolved callees are
    /// reported honestly, the chain is never folded into the ranked brief and is
    /// never a focus-map / ranking input.
    pub symbol_chain: Option<bool>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GuardParams {
    /// Files to report on, project-root-relative. They do not need to exist yet.
    #[schemars(length(min = 1))]
    pub files: Vec<String>,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InspectTarget {
    File {
        #[schemars(length(min = 1))]
        file: String,
    },
    Symbol {
        #[schemars(length(min = 1))]
        file: String,
        #[schemars(length(min = 1))]
        export_name: String,
    },
}

#[derive(Deserialize, JsonSchema)]
pub struct TraceExportParams {
    /// Project-relative path to the file that declares the export.
    #[schemars(length(min = 1))]
    pub file: String,

    /// Name of the exported symbol (or class/enum/store member) to trace.
    #[schemars(length(min = 1))]
    pub export_name: String,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Production mode: exclude test, story, and dev files, only start/build
    /// scripts, and report type-only dependencies.
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SemanticSymbolParams {
    /// Project-relative path to the file that declares the export.
    #[schemars(length(min = 1))]
    pub file: String,

    /// Name of the exported symbol to trace.
    #[schemars(length(min = 1))]
    pub export_name: String,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Explicit tsconfig paths. Project auto-discovery is used when omitted.
    pub type_aware_projects: Option<Vec<String>>,

    /// Whether incomplete semantic evidence is advisory or gating.
    pub type_aware_require: Option<TypeAwareRequireParam>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticExportSelector {
    /// Exact exported symbol name in `file`.
    #[schemars(length(min = 1))]
    pub export_name: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticClassMethodSelector {
    /// Exact exported class name in `file`.
    #[schemars(length(min = 1))]
    pub class_name: String,

    /// Exact method name declared by `class_name`.
    #[schemars(length(min = 1))]
    pub member_name: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SemanticImpactSelector {
    Export(SemanticExportSelector),
    ClassMethod(SemanticClassMethodSelector),
}

#[derive(Deserialize)]
pub struct SemanticImpactParams {
    /// Project-root-relative declaration file.
    pub file: String,

    /// Choose exactly one target shape: `export_name`, or `class_name` plus
    /// `member_name`.
    #[serde(flatten)]
    pub target: SemanticImpactSelector,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Explicit tsconfig paths. Project auto-discovery is used when omitted.
    pub type_aware_projects: Option<Vec<String>>,

    /// Whether incomplete semantic evidence is advisory or gating.
    pub type_aware_require: Option<TypeAwareRequireParam>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

#[expect(
    dead_code,
    reason = "schema-only mirror used by the manual XOR JsonSchema implementation"
)]
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
struct SemanticImpactParamsSchema {
    /// Project-root-relative declaration file.
    #[schemars(length(min = 1))]
    file: String,
    /// Exact exported symbol name. Mutually exclusive with the class-method
    /// selector.
    #[schemars(length(min = 1))]
    export_name: Option<String>,
    /// Exact exported class name. Must be paired with `member_name`.
    #[schemars(length(min = 1))]
    class_name: Option<String>,
    /// Exact method name. Must be paired with `class_name`.
    #[schemars(length(min = 1))]
    member_name: Option<String>,
    /// Project root; defaults to the working directory.
    root: Option<String>,
    /// Path to a fallow config file.
    config: Option<String>,
    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    allow_remote_extends: Option<bool>,
    /// Explicit tsconfig paths. Project auto-discovery is used when omitted.
    type_aware_projects: Option<Vec<String>>,
    /// Whether incomplete semantic evidence is advisory or gating.
    type_aware_require: Option<TypeAwareRequireParam>,
    /// Disable the incremental parse cache.
    no_cache: Option<bool>,
    /// Parser thread count; defaults to CPU cores.
    threads: Option<usize>,
}

impl JsonSchema for SemanticImpactParams {
    fn schema_name() -> Cow<'static, str> {
        "SemanticImpactParams".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = SemanticImpactParamsSchema::json_schema(generator);
        schema.ensure_object().insert(
            "oneOf".to_string(),
            serde_json::json!([
                {
                    "required": ["export_name"],
                    "not": {
                        "anyOf": [
                            {"required": ["class_name"]},
                            {"required": ["member_name"]}
                        ]
                    }
                },
                {
                    "required": ["class_name", "member_name"],
                    "not": {"required": ["export_name"]}
                }
            ]),
        );
        schema
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct TraceFileParams {
    /// Project-relative path to the file to trace.
    #[schemars(length(min = 1))]
    pub file: String,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Production mode: exclude test, story, and dev files, only start/build
    /// scripts, and report type-only dependencies.
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TraceImportPathParams {
    /// Project-relative path of the module the walk starts from.
    #[schemars(length(min = 1))]
    pub from: String,

    /// Project-relative path of the module the walk is looking for.
    #[schemars(length(min = 1))]
    pub to: String,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Production mode: exclude test, story, and dev files, only start/build
    /// scripts, and report type-only dependencies.
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TraceErrorParams {
    /// The runtime stack trace text, pasted verbatim.
    #[schemars(length(min = 1))]
    pub trace: String,

    /// Where the trace came from, echoed back as the payload's `source`.
    pub source: Option<String>,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Production mode: exclude test, story, and dev files, only start/build
    /// scripts, and report type-only dependencies.
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ImpactClosureParams {
    /// Project-relative path to the file whose impact closure to trace.
    #[schemars(length(min = 1))]
    pub path: String,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Production mode: exclude test, story, and dev files, only start/build
    /// scripts, and report type-only dependencies.
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct TraceDependencyParams {
    /// Name of the npm package to trace usage for.
    #[schemars(length(min = 1))]
    pub package_name: String,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Production mode: exclude test, story, and dev files, only start/build
    /// scripts, and report type-only dependencies.
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Add the per-name `usage` counts.
    pub usage: Option<bool>,

    /// Imported names to report. Implies `sites`.
    pub specifiers: Option<Vec<String>>,

    /// Add one page of usage sites. Implies `usage`.
    pub sites: Option<bool>,

    /// Sites per page (default 50). Implies `sites`.
    #[schemars(range(min = 1, max = 500))]
    pub limit: Option<u16>,

    /// `usage.sites.next_cursor` of the previous page.
    pub cursor: Option<String>,

    /// Add importers of the users, up to this depth. Implies `usage`.
    #[schemars(range(min = 1, max = 10))]
    pub closure_depth: Option<u32>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct TraceCloneParams {
    /// Project-relative path to a file containing the clone instance. Used
    /// with `line` to address by source location; omit when using `fingerprint`.
    #[serde(default)]
    pub file: Option<String>,

    /// Line number of the clone instance within `file`. Used with `file` to
    /// address by source location; omit when using `fingerprint`.
    #[serde(default)]
    pub line: Option<usize>,

    /// Stable clone-group identity (`dup:<id>`) from a `find_dupes`
    /// `clone_groups[].fingerprint`. Alternative to addressing by `file` plus `line`.
    #[serde(default)]
    pub fingerprint: Option<String>,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Detection mode: "strict", "mild", "weak", or "semantic". Defaults to
    /// the project config, or "mild" if unset.
    pub mode: Option<String>,

    /// Detect function-scoped near-miss clones before resolving the trace.
    pub near: Option<bool>,

    /// Minimum token count for a clone. Defaults to the project config, or 50 if unset.
    pub min_tokens: Option<u32>,

    /// Minimum line count for a clone. Defaults to the project config, or 5 if unset.
    pub min_lines: Option<u32>,

    /// Minimum number of occurrences before a clone group is reported.
    /// Defaults to the project config, or 2 if unset.
    #[schemars(range(min = 2))]
    pub min_occurrences: Option<u32>,

    /// Fail if duplication exceeds this percentage (0 = no limit). Defaults
    /// to the project config, or 0 if unset.
    pub threshold: Option<f64>,

    /// Only report cross-directory duplicates.
    pub skip_local: Option<bool>,

    /// Omit clone instances whose path is a symlink or lies under a symlinked
    /// directory. Defaults to the project config; set `false` to report them
    /// even when the config sets `ignoreSymlinks`.
    pub ignore_symlinks: Option<bool>,

    /// Enable cross-language detection (strips TypeScript type annotations for TS/JS matching).
    pub cross_language: Option<bool>,

    /// Exclude import declarations from clone detection. Defaults to `true`
    /// (sorted import blocks are a formatting artifact, not copy-paste); set
    /// `false` to count them again.
    pub ignore_imports: Option<bool>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,
}
#[path = "params_health.rs"]
mod health_params;
pub use health_params::*;

/// Parameters for `check_runtime_coverage`, the focused runtime-coverage
/// entry point. A thin wrapper around `fallow health --runtime-coverage
/// <path>` with a narrow surface area so agents can pick the right tool
/// by name and pass exactly the knobs that apply to runtime coverage. A
/// single local capture is free and runs without a license; continuous or
/// multi-capture runtime monitoring (a directory containing multiple JSON
/// dumps) requires an active license JWT (`fallow license activate
/// --trial --email <addr>` to start a 30-day trial). Long V8 dumps can
/// exceed the default 120s MCP subprocess timeout; raise
/// `FALLOW_TIMEOUT_SECS` for multi-megabyte inputs.
#[derive(Deserialize, JsonSchema)]
pub struct CheckRuntimeCoverageParams {
    /// Path to runtime coverage input. Accepts a V8 coverage directory
    /// (`NODE_V8_COVERAGE=<dir>`), a single V8 coverage JSON file, or an
    /// Istanbul `coverage-final.json`. Required.
    pub coverage: String,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Minimum invocation count for a function to be classified as a hot
    /// path. Inherits the CLI default (100) when omitted.
    pub min_invocations_hot: Option<u64>,

    /// Minimum total trace volume before the sidecar may emit high-confidence
    /// `safe_to_delete` or `review_required` verdicts. Below this threshold,
    /// confidence is capped at `medium` to protect against overconfident
    /// verdicts on new or low-traffic services. Inherits the sidecar default
    /// (5000) when omitted.
    pub min_observation_volume: Option<u32>,

    /// Fraction of `trace_count` below which an invoked function is
    /// classified `low_traffic` rather than `active`. Expressed as a
    /// decimal (0.001 = 0.1%). Inherits the sidecar default (0.001) when
    /// omitted.
    pub low_traffic_threshold: Option<f64>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Maximum CRAP score threshold (default 30.0). Functions meeting or
    /// exceeding this score appear as findings alongside complexity violations.
    /// Production V8 coverage yields the most accurate per-function CRAP
    /// inputs, making this flag especially useful on this tool.
    pub max_crap: Option<f64>,

    /// Show only the top N runtime findings, hot paths, file scores, and
    /// refactoring targets. Passed through to the CLI's `--top` flag.
    pub top: Option<usize>,

    /// Group health findings by CODEOWNERS ownership, directory, workspace
    /// package, or GitLab CODEOWNERS section. Values: "owner", "directory",
    /// "package", "section". `section` attaches an `owners: string[]` array
    /// to each group. Passed through to the CLI's `--group-by` flag.
    pub group_by: Option<String>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

/// Parameters for `get_cloud_runtime_context`, the cloud-backed sibling of
/// `check_runtime_coverage`. There is no `coverage` path because the runtime
/// facts come from Fallow Cloud, and no API-key field because the key does not
/// cross the wire on every call. The CLI reads `FALLOW_API_KEY` in the server
/// environment, else the key that `fallow cloud login` stored.
#[derive(Default, Deserialize, JsonSchema)]
pub struct CloudRuntimeContextParams {
    /// Repository Fallow Cloud holds runtime facts for, as `owner/repo`.
    /// Required.
    pub repo: String,

    /// Project disambiguator when one repository reports several projects.
    pub project_id: Option<String>,

    /// Runtime observation window in days, 1 to 90. Inherits the CLI default
    /// (30) when omitted.
    pub period_days: Option<u16>,

    /// Runtime environment filter, for example `production`.
    pub environment: Option<String>,

    /// Commit SHA filter for the runtime facts the cloud returns.
    pub commit_sha: Option<String>,

    /// Project root; defaults to the working directory. It must be a checkout
    /// of `repo`: cloud functions are joined against its static analysis.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Minimum invocation count for a function to be classified as a hot
    /// path. Inherits the CLI default (100) when omitted.
    pub min_invocations_hot: Option<u64>,

    /// Show only the top N runtime findings and hot paths.
    pub top: Option<usize>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

/// One function target of `get_cloud_review_packet`.
#[derive(Default, Deserialize, JsonSchema)]
pub struct CloudFunctionTarget {
    /// Repo-relative file.
    pub file: String,

    /// Function name.
    pub name: String,

    /// Start line.
    pub line: Option<u32>,
}

/// Parameters for `get_cloud_review_packet`. The API key comes from
/// `FALLOW_API_KEY` in the server environment, else from `fallow cloud login`.
#[derive(Default, Deserialize, JsonSchema)]
pub struct CloudReviewPacketParams {
    /// Repository as `owner/repo`. Required.
    pub repo: String,

    /// Repo-relative files, at most 1000.
    pub files: Option<Vec<String>>,

    /// Functions, at most 1000.
    pub functions: Option<Vec<CloudFunctionTarget>>,

    /// Window in days, 1 to 90.
    pub period_days: Option<u16>,

    /// Project disambiguator.
    pub project_id: Option<String>,

    /// Deployment commit SHA.
    pub commit_sha: Option<String>,

    /// Base ref for the changed-file default.
    pub base: Option<String>,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

/// Parameters for `get_cloud_deployment_changes`. The API key comes from
/// `FALLOW_API_KEY` in the server environment, else from `fallow cloud login`.
#[derive(Default, Deserialize, JsonSchema)]
pub struct CloudDeploymentChangesParams {
    /// Repository as `owner/repo`. Required.
    pub repo: String,

    /// Deployment commit SHA; defaults to `HEAD`.
    pub sha: Option<String>,

    /// Base deployment commit SHA.
    pub base: Option<String>,

    /// One change kind to keep.
    pub change: Option<String>,

    /// Page size, 1 to 200.
    pub limit: Option<u16>,

    /// `meta.cursor` of the previous page.
    pub cursor: Option<String>,

    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct AuditParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Git ref to compare against (e.g., "main", "HEAD~5"). When unset, the
    /// base is the git merge-base against the branch's upstream or the remote
    /// default (`origin/main`); set `FALLOW_AUDIT_BASE` in the server env to pin
    /// it.
    pub base: Option<String>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Run only the dead-code sub-analysis in production mode.
    pub production_dead_code: Option<bool>,

    /// Run only the health sub-analysis in production mode.
    pub production_health: Option<bool>,

    /// Run only the duplication sub-analysis in production mode.
    pub production_dupes: Option<bool>,

    /// Run only these audit sub-analyses, with the values of bare `fallow
    /// --only`: `dead-code`, `architecture`, `dupes`, `health`. Replaces the
    /// `audit.deadCode`, `audit.health` and `audit.dupes` config selection.
    /// Cannot be combined with `skip`.
    pub only: Option<Vec<String>>,

    /// Skip these audit sub-analyses, with the values of `only`. A skipped
    /// sub-analysis leaves the verdict; the response omits its block and sets
    /// `summary.dead_code_skipped`, `complexity_skipped` or
    /// `duplication_skipped`. A baseline for a skipped sub-analysis is an
    /// error.
    pub skip: Option<Vec<String>>,

    /// Set `false` to skip styling analytics in audit. Unset or `true` uses
    /// `audit.css` from the config, which defaults to enabled.
    pub css: Option<bool>,

    /// Enable or disable deep CSS analysis for audit: project-wide styling
    /// reachability and near-duplicate theme-token candidates, narrowed back to
    /// changed anchors. Unset uses `audit.cssDeep` from the config, which
    /// defaults to enabled; `true` or `false` overrides the config. No effect
    /// when styling analytics are off.
    pub css_deep: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Refine both audit sides with TypeScript symbol identity before attribution.
    /// `false` keeps the run syntactic, also when the config turns type-aware analysis on. `type_aware_projects` or `type_aware_require` turns it on and wins over `false`.
    pub type_aware: Option<bool>,

    /// Explicit tsconfig paths for type-aware analysis.
    pub type_aware_projects: Option<Vec<String>>,

    /// Whether incomplete semantic evidence is advisory or gating.
    pub type_aware_require: Option<TypeAwareRequireParam>,

    /// Group audit findings by CODEOWNERS ownership, directory, workspace
    /// package, or GitLab CODEOWNERS section. Values: "owner", "directory",
    /// "package", "section". `section` attaches an `owners: string[]` array
    /// to each group in the JSON output. Passed through to the CLI's
    /// `--group-by` flag; propagates to all three sub-analyses (dead-code,
    /// dupes, health) that audit runs.
    pub group_by: Option<String>,

    /// Which findings affect the audit verdict. Values: "new-only" or "all".
    /// Unset uses `audit.gate` from the config, which defaults to "new-only".
    /// Passed through to the CLI's `--gate` flag.
    pub gate: Option<String>,

    /// Verdict effect of an inherited complexity finding whose measured value
    /// grew: "warn" (default), "error" (counts as introduced) or "off". Unset uses
    /// `audit.worsened`.
    pub worsened: Option<String>,

    /// Path to a dead-code baseline file (produced by `fallow dead-code
    /// --save-baseline`). When set, dead-code issues present in the
    /// baseline are excluded from the audit verdict. Passed through to
    /// the CLI's `--dead-code-baseline` flag.
    pub dead_code_baseline: Option<String>,

    /// Path to a health baseline file (produced by `fallow health
    /// --save-baseline`). When set, complexity findings present in the
    /// baseline are excluded from the audit verdict. Passed through to
    /// the CLI's `--health-baseline` flag.
    pub health_baseline: Option<String>,

    /// How `health_baseline` matches complexity findings: "count" (per file
    /// and category, the CLI default) or "identity" (per function identity,
    /// strict; requires a baseline saved with `--baseline-mode identity`).
    /// Passed through to the CLI's `--baseline-mode` flag.
    pub health_baseline_mode: Option<BaselineModeParam>,

    /// Path to a duplication baseline file (produced by `fallow dupes
    /// --save-baseline`). When set, clone groups present in the baseline
    /// are excluded from the audit verdict. Passed through to the CLI's
    /// `--dupes-baseline` flag.
    pub dupes_baseline: Option<String>,

    /// Show per-pattern counts for skipped files: the default duplicates
    /// ignores, and the built-in discovery ignores that removed source files
    /// from the walk.
    /// Human-format only (human/markdown CLI output); MCP JSON responses suppress the note.
    pub explain_skipped: Option<bool>,

    /// Maximum CRAP score threshold (default 30.0). Functions meeting or
    /// exceeding this score cause audit to fail. Pair with `coverage` on
    /// `check_health` for accurate per-function CRAP; without Istanbul data
    /// fallow estimates coverage from the module graph. Passed through to
    /// the CLI's `--max-crap` flag.
    pub max_crap: Option<f64>,

    /// Path to Istanbul coverage data (coverage-final.json) or raw V8 coverage
    /// (a `NODE_V8_COVERAGE` directory or one V8 JSON file) for accurate
    /// per-function CRAP scores in audit's health sub-analysis.
    /// When omitted, `FALLOW_COVERAGE` and then the `health.coverage` config
    /// field apply, on the typed route and on the CLI fallback alike (where
    /// the value is forwarded as `--coverage`), with the same precedence as
    /// `fallow audit --coverage`.
    pub coverage: Option<String>,

    /// Absolute prefix to strip from coverage data paths before CRAP matching.
    /// Use when coverage was generated in a different checkout root in CI or Docker.
    /// When omitted, `FALLOW_COVERAGE_ROOT` and then the `health.coverageRoot`
    /// config field apply, on the typed route and on the CLI fallback alike
    /// (where the value is forwarded as `--coverage-root`), with the same
    /// precedence as `fallow audit --coverage-root`.
    pub coverage_root: Option<String>,

    /// Report unused exports in entry files instead of auto-marking them as
    /// used. Catches typos in framework exports (e.g. `meatdata` instead of
    /// `metadata`). Also configurable persistently via
    /// `includeEntryExports: true` in the fallow config file; this param
    /// ORs with the config value. Passed through to the CLI's
    /// `--include-entry-exports` flag.
    pub include_entry_exports: Option<bool>,

    /// Keep the unused exports, types, class members and enum members of
    /// unused files. By default the report hides them and counts them in
    /// `cascade_hidden`. Passed through to the CLI's `--show-cascade` flag.
    pub show_cascade: Option<bool>,

    /// Runtime coverage input (V8 directory, V8 JSON, or Istanbul coverage
    /// map JSON). When set, audit adds runtime-coverage findings to the same
    /// call: agents get the `hot-path-touched` verdict next to dead-code and
    /// complexity in one MCP call, without a second `check_runtime_coverage`
    /// step. The verdict is informational. A single local capture is free.
    /// Continuous or multi-capture monitoring needs a license. Passed
    /// through to the CLI's `--runtime-coverage` flag.
    pub runtime_coverage: Option<String>,

    /// Threshold for hot-path classification (default 100). Forwarded to
    /// the sidecar when `runtime_coverage` is set. Passed through to the
    /// CLI's `--min-invocations-hot` flag.
    pub min_invocations_hot: Option<u64>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct ExplainParams {
    /// Issue type or rule id to explain, for example "unused-export",
    /// "fallow/unused-dependency", "high-complexity", or "code-duplication".
    pub issue_type: String,
}

/// Parameters for `list_boundaries`.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct ListBoundariesParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,
}

/// Parameters for the `recommend` config-recommendation tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct RecommendParams {
    /// Project root for framework, workspace, and tooling detection; defaults to the working directory.
    pub root: Option<String>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

/// Parameters for the `impact` value-report tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ImpactParams {
    /// Project root whose local value history to read (stored outside the repo); defaults to the working directory.
    pub root: Option<String>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

/// Parameters for the `impact_all` cross-repo value-report tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ImpactAllParams {
    /// Row ordering: `recent` (default, most recently recorded project first),
    /// `resolved` (most findings resolved first), `contained` (most commits
    /// contained first), or `name` (alphabetical by project label).
    pub sort: Option<String>,

    /// Cap the number of project rows returned. Grand totals still reflect
    /// every tracked project, including any beyond the cap. Omit for all rows.
    pub limit: Option<usize>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CodeExecuteParams {
    /// JavaScript function expression or function body. The function receives
    /// `{ fallow, root }` and must return a JSON-serializable value.
    #[schemars(length(min = 1, max = 20000))]
    pub code: String,

    /// Default project root injected into fallow host calls when their params
    /// omit `root`.
    pub root: Option<String>,

    /// Overall sandbox timeout in milliseconds. Defaults to 5000 and is capped
    /// at 30000.
    #[schemars(range(min = 1, max = 30000))]
    pub timeout_ms: Option<u64>,

    /// Maximum total bytes of fallow JSON output that sandbox host calls may
    /// read. Defaults to 1000000 and is capped at 4000000.
    #[schemars(range(min = 1024, max = 4_000_000))]
    pub max_output_bytes: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct FeatureFlagsParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Show only the top N flags.
    pub top: Option<usize>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Add the `retirement` block: one row per flag with reasons, git age and evidence.
    pub retirement: Option<bool>,

    /// Offline vendor flag export for the vendor reasons; relative to `root`. Needs `retirement`.
    pub flag_state: Option<String>,

    /// Flag age: "blame" (default, lower bound), "pickaxe" or "off". Needs `retirement`.
    pub flag_age: Option<String>,
}

/// Parameters for the `list_suppressions` governance inventory tool. Wraps
/// `fallow suppressions --format json`, returning the `suppression-inventory`
/// envelope verbatim. `--changed-workspaces` is deliberately not forwarded in
/// v1 (niche; `workspace` plus `changed_since` cover the agent use cases).
#[derive(Default, Deserialize, JsonSchema)]
pub struct ListSuppressionsParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Git ref (e.g. "main", "HEAD~5"). Scopes the inventory to files changed
    /// since the ref, the natural pull-request review scope.
    pub changed_since: Option<String>,

    /// Only list suppressions in these files. Relative paths resolve against the
    /// project root; forwarded as a repeated `--file` flag.
    pub file: Option<Vec<String>>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct DecisionSurfaceParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Git ref to compare against (e.g., "main", "HEAD~5"). When unset, the
    /// base is the git merge-base against the branch's upstream or the remote
    /// default (`origin/main`); set `FALLOW_AUDIT_BASE` in the server env to pin
    /// it.
    pub base: Option<String>,

    /// Cap on the number of consequential structural decisions surfaced (the
    /// working-memory limit). Default 4; clamped to the 3-5 band (4 plus or minus
    /// 1).
    pub max_decisions: Option<usize>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,
}

/// Parameters for `get_token_blast_radius`, a thin wrapper around
/// `fallow health --css --format json` that steers agents to the
/// `css_analytics.token_consumers` reverse index (Tailwind v4 `@theme` tokens
/// plus CSS-in-JS `defineVars` / `createTheme`-family token definitions). Narrow
/// surface: only root/config plus the global cache knobs apply. `token_consumers`
/// abstains on partial scope, so `workspace` / `changed_since` are intentionally
/// omitted (they would only ever return empty).
#[derive(Default, Deserialize, JsonSchema)]
pub struct GetTokenBlastRadiusParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Byte cap for this call's response. Lowers the 16 MiB default; over it
    /// the call is REFUSED (`isError`, `exit_code: 2`) with only a preview.
    pub max_output_bytes: Option<usize>,
}
