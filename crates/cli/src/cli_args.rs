//! Clap definition of the `fallow` command line.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::cli_agent::AgentCli;
use crate::cli_format::Format;
use crate::cli_hooks::HooksCli;
use crate::cli_impact::{ImpactCli, ImpactSortCli};
use crate::dispatch::resolve_negatable_flag;
use crate::dupes::{DupesMode, DupesOverrides};
use crate::health::SortBy;
use crate::runtime_support::{AnalysisKind, GroupBy};
#[path = "cli_args_min_occurrences.rs"]
mod min_occurrences;
use crate::{
    audit_decision_surface, doctor, flags, mutation_cli, security, setup_hooks, similar_code_cli,
    viz,
};
use min_occurrences::parse_min_occurrences;

#[path = "cli_args_root.rs"]
mod root_args;
pub use root_args::{Cli, TOP_LEVEL_AFTER_LONG_HELP};

#[path = "cli_args_overrides.rs"]
mod overrides;

#[derive(Subcommand)]
pub enum Command {
    /// Analyze project for unused code and dependency hygiene
    #[command(name = "dead-code", alias = "check")]
    Check {
        /// Only report unused files
        #[arg(long, help_heading = "Issue types")]
        unused_files: bool,

        /// Only report unused exports
        #[arg(long, help_heading = "Issue types")]
        unused_exports: bool,

        /// Only report dependency findings: unused dependencies, dev dependencies and optional dependencies, plus type-only, test-only and dev-in-production dependencies
        #[arg(long, help_heading = "Issue types")]
        unused_deps: bool,

        /// Only report unused type exports
        #[arg(long, help_heading = "Issue types")]
        unused_types: bool,

        /// Opt in to private type leak API hygiene findings and only report that issue type
        #[arg(long, help_heading = "Issue types")]
        private_type_leaks: bool,

        /// Opt in to `@deprecated` exports that are still in use and only report that issue type
        #[arg(long, help_heading = "Issue types")]
        deprecated_exports_in_use: bool,

        /// Only report unused enum members
        #[arg(long, help_heading = "Issue types")]
        unused_enum_members: bool,

        /// Only report unused class members
        #[arg(long, help_heading = "Issue types")]
        unused_class_members: bool,

        /// Only report unused store members
        #[arg(long, help_heading = "Issue types")]
        unused_store_members: bool,

        /// Only report unused namespace members
        #[arg(long, help_heading = "Issue types")]
        unused_namespace_members: bool,

        /// Only report unused protected class members
        #[arg(long, help_heading = "Issue types")]
        unused_protected_class_members: bool,

        /// Only report unprovided injects
        #[arg(long, help_heading = "Issue types")]
        unprovided_injects: bool,

        /// Only report unrendered components
        #[arg(long, help_heading = "Issue types")]
        unrendered_components: bool,

        /// Only report unused component props
        #[arg(long, help_heading = "Issue types")]
        unused_component_props: bool,

        /// Review optional props omitted by known reachable callers (enables this rule)
        #[arg(long, help_heading = "Issue types")]
        absent_component_props: bool,

        /// Review parameters that each visible call passes in one way (enables this rule)
        #[arg(long, help_heading = "Issue types")]
        constant_parameters: bool,

        /// Only report unused component emits
        #[arg(long, help_heading = "Issue types")]
        unused_component_emits: bool,

        /// Only report unused component inputs
        #[arg(long, help_heading = "Issue types")]
        unused_component_inputs: bool,

        /// Only report unused component outputs
        #[arg(long, help_heading = "Issue types")]
        unused_component_outputs: bool,

        /// Only report unused Svelte dispatched events
        #[arg(long, help_heading = "Issue types")]
        unused_svelte_events: bool,

        /// Only report Effect layers composed only into unused layers
        #[arg(long, help_heading = "Issue types")]
        unused_effect_layers: bool,

        /// Only report Effect tagged errors that no code produces
        #[arg(long, help_heading = "Issue types")]
        unused_tagged_errors: bool,

        /// Only report catchTag handlers for a tag that no code produces
        #[arg(long, help_heading = "Issue types")]
        unproduced_catch_tags: bool,

        /// Only report unused server actions
        #[arg(long, help_heading = "Issue types")]
        unused_server_actions: bool,

        /// Only report unused SvelteKit load() data keys
        #[arg(long, help_heading = "Issue types")]
        unused_load_data_keys: bool,

        /// Only report unresolved imports
        #[arg(long, help_heading = "Issue types")]
        unresolved_imports: bool,

        /// Only report unlisted dependencies
        #[arg(long, help_heading = "Issue types")]
        unlisted_deps: bool,

        /// Only report binaries that no installed package provides
        #[arg(long, help_heading = "Issue types")]
        unlisted_binaries: bool,

        /// Only report duplicate exports
        #[arg(long, help_heading = "Issue types")]
        duplicate_exports: bool,

        /// Only report bindings that one file exports under two or more names
        #[arg(long, help_heading = "Issue types")]
        duplicate_export_aliases: bool,

        /// Only report circular dependencies.
        /// Deprecated alias: use `fallow architecture --cycles`.
        #[arg(long, help_heading = "Issue types")]
        circular_deps: bool,

        /// Only report re-export cycles.
        /// Deprecated alias: use `fallow architecture --cycles`.
        #[arg(long, help_heading = "Issue types")]
        re_export_cycles: bool,

        /// Only report dependency cycles between workspace packages.
        /// Deprecated alias: use `fallow architecture --cycles`.
        #[arg(long, help_heading = "Issue types")]
        package_cycles: bool,

        /// Only report boundary violations.
        /// Deprecated alias: use `fallow architecture --boundaries`.
        #[arg(long, help_heading = "Issue types")]
        boundary_violations: bool,

        /// Only report rule-pack policy violations.
        /// Deprecated alias: use `fallow architecture --policy`.
        #[arg(long, help_heading = "Issue types")]
        policy_violations: bool,

        /// Only report stale suppressions
        #[arg(long, help_heading = "Issue types")]
        stale_suppressions: bool,

        /// Only report unused pnpm catalog entries
        #[arg(long, help_heading = "Issue types")]
        unused_catalog_entries: bool,

        /// Only report empty pnpm catalog groups
        #[arg(long, help_heading = "Issue types")]
        empty_catalog_groups: bool,

        /// Only report unresolved pnpm catalog references
        #[arg(long, help_heading = "Issue types")]
        unresolved_catalog_references: bool,

        /// Only report unused package-manager dependency overrides
        #[arg(long, help_heading = "Issue types")]
        unused_dependency_overrides: bool,

        /// Only report misconfigured package-manager dependency overrides
        #[arg(long, help_heading = "Issue types")]
        misconfigured_dependency_overrides: bool,

        /// Also run duplication analysis and cross-reference with dead code
        #[arg(long, help_heading = "Issue types")]
        include_dupes: bool,

        /// Trace why an export is used/unused (format: `FILE:EXPORT_NAME`)
        #[arg(long, value_name = "FILE:EXPORT", help_heading = "Tracing")]
        trace: Option<String>,

        /// Trace all edges for a file (imports, exports, importers)
        #[arg(long, value_name = "PATH", help_heading = "Tracing")]
        trace_file: Option<String>,

        /// Trace where a dependency is used
        #[arg(long, value_name = "PACKAGE", help_heading = "Tracing")]
        trace_dependency: Option<String>,

        /// Compute the impact closure for a file (the transitive
        /// affected-but-not-in-diff set + coordination gap). Walks reverse-deps
        /// and re-export chains. `fallow inspect` reports the same closure.
        #[arg(long, value_name = "PATH", help_heading = "Tracing")]
        impact_closure: Option<String>,

        /// Compute exact-symbol consumers, affected files, and targeted tests.
        #[arg(long, value_name = "FILE:EXPORT", help_heading = "Tracing")]
        symbol_impact: Option<String>,

        /// Only report issues in the specified file(s). Accepts multiple values.
        /// The full project graph is still built, but only issues in matching files
        /// are reported. Useful for lint-staged pre-commit hooks.
        #[arg(long, value_name = "PATH", help_heading = "Scope")]
        file: Vec<std::path::PathBuf>,

        /// Only report the findings with these `finding_id` values. Repeat the flag
        /// or pass a comma-separated list. Ids stay the same under every filter.
        /// The JSON output adds `finding_id_query`: a missing id means "resolved"
        /// only when `conclusive` is true.
        #[arg(
            long = "finding-id",
            value_name = "ID",
            value_delimiter = ',',
            help_heading = "Scope"
        )]
        finding_id: Vec<String>,

        /// Show only the top N items per category. Human output only: the JSON,
        /// SARIF, and CodeClimate envelopes drive exit codes and CI baselines,
        /// so they always carry every finding and a consumer slices the arrays
        /// itself.
        #[arg(long, help_heading = "Output")]
        top: Option<usize>,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },

    /// Check import cycles, boundaries and policy rules after editing
    ///
    /// Run `fallow guard <files>` before an edit to see the rules that apply to
    /// those files.
    Architecture {
        /// Inventory direct imported writes and custom global access sites
        #[arg(long)]
        shared_state: bool,
        /// Only report import cycles: circular dependencies, re-export cycles
        /// and package cycles
        #[arg(long)]
        cycles: bool,

        /// Only report boundary violations, boundary coverage and forbidden calls
        #[arg(long)]
        boundaries: bool,

        /// Only report rule-pack policy violations
        #[arg(long)]
        policy: bool,

        /// Only report issues in the specified file(s). Accepts multiple values.
        /// The full project graph is still built, but only issues in matching files
        /// are reported.
        #[arg(long, value_name = "PATH")]
        file: Vec<std::path::PathBuf>,

        /// Only report the findings with these `finding_id` values. Repeat the flag
        /// or pass a comma-separated list.
        #[arg(long = "finding-id", value_name = "ID", value_delimiter = ',')]
        finding_id: Vec<String>,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },

    /// Watch for changes and re-run analysis
    Watch {
        /// Do not clear the screen between re-analyses
        #[arg(long)]
        no_clear: bool,
    },

    /// Inspect the optional type-aware companion for TypeScript semantic analysis.
    TypeAware {
        #[command(subcommand)]
        subcommand: TypeAwareCli,
    },

    /// Diagnose project readiness without analysis or mutation.
    ///
    /// Checks the root, config resolution, workspace discovery, external
    /// plugins, and the optional type-aware companion. Uses only local reads;
    /// it performs no cache writes, telemetry, network requests, or third-party
    /// execution. Supports human and JSON output.
    #[command(override_help = doctor::HELP)]
    Doctor,

    /// Find semantically similar functions with a pinned local model (opt-in).
    ///
    /// Results are unverified candidates. The model score is not a probability,
    /// gate, vulnerability verdict, or safe-refactor decision. Model setup is
    /// explicit, and project source remains local and offline during analysis.
    SimilarCode {
        #[command(subcommand)]
        subcommand: Option<similar_code_cli::SimilarCodeSubcommand>,
        /// Minimum cosine similarity retained as an unverified candidate.
        #[arg(long, value_name = "0..1")]
        threshold: Option<f64>,
        /// Minimum source lines per extracted function.
        #[arg(long, value_name = "N")]
        min_lines: Option<usize>,
        /// Cap displayed candidates after bounded full-corpus comparison.
        #[arg(long, value_name = "N")]
        top: Option<usize>,
        /// Report pairs touching one of these project-relative files.
        #[arg(long, value_name = "PATH")]
        file: Vec<PathBuf>,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// Measure how well the tests detect changes in the code (opt-in).
    ///
    /// Changes the code in small ways (mutants) and runs the project tests
    /// against each mutant. A mutant that no test fails on survives. The
    /// default scope is the changed code against the base ref, as `audit`.
    /// `--plan` and `inspect` run no code. Fallow does not write to project
    /// files. The report lists any path that the tests changed.
    #[command(disable_help_subcommand = true)]
    Mutation {
        #[command(subcommand)]
        subcommand: Option<mutation_cli::MutationSubcommand>,
        #[command(flatten)]
        args: mutation_cli::MutationArgs,
    },

    /// Inspect one file or exported symbol as a bundled evidence query
    Inspect {
        /// File to inspect.
        #[arg(
            long,
            value_name = "PATH",
            conflicts_with = "symbol",
            required_unless_present = "symbol"
        )]
        file: Option<String>,

        /// Exported symbol to inspect, formatted as FILE:EXPORT.
        #[arg(long, value_name = "FILE:EXPORT", conflicts_with = "file")]
        symbol: Option<String>,

        /// Opt-in: also attach the best-effort symbol-level call chain
        /// (`fallow trace`) as the `symbol_chain` evidence section. Only
        /// meaningful for a `--symbol` target. Default off (best-effort,
        /// syntactic, not part of the ranked brief).
        #[arg(long)]
        symbol_chain: bool,

        /// Opt-in: attach target-level git churn evidence from the health
        /// hotspot subsystem. Default off to avoid git-history latency.
        #[arg(long)]
        churn: bool,

        /// Opt-in: attach Fallow Cloud production evidence for a `--file`
        /// target as the `cloud` evidence section: calls, errors, CPU and
        /// never-run branches per function. The API key is $FALLOW_API_KEY,
        /// else the key that `fallow cloud login` stored. A missing key, a
        /// plan gate or an unreachable cloud is a section status, never a
        /// failed run.
        #[arg(long, conflicts_with = "symbol")]
        cloud: bool,

        /// Fallow Cloud repository for `--cloud`. Precedence: this flag >
        /// $FALLOW_REPO > the `origin` git remote.
        #[arg(long, value_name = "OWNER/REPO", requires = "cloud")]
        repo: Option<String>,
    },

    /// Connect this machine to Fallow Cloud.
    ///
    /// `login` runs the device flow: it shows a code and a URL, and after
    /// you approve the code in the dashboard it stores a read-only API key
    /// in the OS keychain (service `fallow-cloud`), or in
    /// `~/.config/fallow/credentials` (mode 0600) when no keychain is
    /// available. `--api-key` and $FALLOW_API_KEY still win over the
    /// stored key. `logout` removes the stored key.
    Cloud {
        #[command(subcommand)]
        subcommand: CloudCli,
    },

    /// Trace a symbol's call chain, or the shortest import path between two
    /// modules (best-effort, syntactic; not part of the ranked brief)
    ///
    /// Walks callers up (modules that import the symbol) and callees down
    /// (import-symbol edges + intra-module call sites) via the module graph,
    /// bounded by `--depth`. Symbol-level chains are labeled best-effort,
    /// because the analysis is syntactic: resolved and unresolved callees are
    /// both reported, never silently dropped. The result is a separate surface:
    /// it is not part of the ranked brief and never feeds the focus map.
    ///
    /// `--path <FROM> <TO>` answers a different question: the shortest chain of
    /// imports by which one module reaches another. Type-only hops are reported
    /// rather than skipped, and an unreachable pair reports `reachable: false`
    /// instead of an error.
    Trace {
        /// Target symbol, formatted as FILE:SYMBOL (e.g. src/utils.ts:formatDate).
        /// Omitted when `--path` or `--dependency` is used.
        #[arg(
            value_name = "FILE:SYMBOL",
            required_unless_present_any = ["path", "dependency"]
        )]
        symbol: Option<String>,

        /// How the code uses each imported name of a package: file and call
        /// counts, one hop through project wrappers, and a count of each use
        /// that the trace cannot resolve
        ///
        /// `--sites`, `--specifier`, `--limit` and `--cursor` page the usage
        /// sites. `--callers --depth N` adds the files that import the users.
        #[arg(
            long,
            value_name = "PACKAGE",
            conflicts_with_all = ["symbol", "path", "callees"],
            help_heading = "Tracing",
        )]
        dependency: Option<String>,

        /// With `--dependency`, report only these imported names
        /// (repeatable or comma-separated). Turns on the site page.
        #[arg(
            long,
            value_name = "NAME",
            value_delimiter = ',',
            requires = "dependency",
            help_heading = "Scope"
        )]
        specifier: Vec<String>,

        /// With `--dependency`, list the usage sites with file, line and column
        #[arg(long, requires = "dependency", help_heading = "Output")]
        sites: bool,

        /// With `--dependency`, the largest number of sites on a page
        /// (1 to 500, default 50). Turns on the site page.
        #[arg(
            long,
            value_name = "N",
            requires = "dependency",
            value_parser = clap::value_parser!(u16).range(1..=500),
            help_heading = "Output",
        )]
        limit: Option<u16>,

        /// With `--dependency`, the `next_cursor` of the previous page. Turns
        /// on the site page.
        #[arg(
            long,
            value_name = "TOKEN",
            requires = "dependency",
            help_heading = "Output"
        )]
        cursor: Option<String>,

        /// Shortest import path between two modules, as two file paths
        /// (e.g. `--path src/app.ts src/db.ts`). Mutually exclusive with the
        /// symbol target and the call-chain flags.
        #[arg(
            long,
            num_args = 2,
            value_names = ["FROM", "TO"],
            conflicts_with_all = ["symbol", "callers", "callees", "depth"],
            help_heading = "Tracing",
        )]
        path: Vec<String>,

        /// With `--path`, follow only static value imports, so the route
        /// explains why TO loads before FROM runs
        ///
        /// `import()`, lazy globs, worker loads and `import type` do not
        /// qualify.
        #[arg(long, requires = "path", help_heading = "Tracing")]
        eager_only: bool,

        /// Walk up to callers (modules that import the symbol). When neither
        /// `--callers` nor `--callees` is set, both directions are walked.
        #[arg(long, help_heading = "Tracing")]
        callers: bool,

        /// Walk down to callees (the symbol's module's import-symbol edges plus
        /// unresolved call sites). When neither flag is set, both are walked.
        #[arg(long, help_heading = "Tracing")]
        callees: bool,

        /// Chain depth bound for both directions (default 2). Symbol-level is
        /// best-effort, so a shallow bound keeps the trace legible.
        #[arg(long, value_name = "N", help_heading = "Tracing")]
        depth: Option<u32>,
    },

    /// Resolve a runtime stack trace's frames to the definitions they name
    /// (best-effort, syntactic; OFF the ranked path)
    ///
    /// Reads a V8 / Node or SpiderMonkey / JavaScriptCore stack trace from a
    /// file or from stdin, classifies every frame as in-project,
    /// node-modules, or out-of-corpus, and asks the module graph which
    /// definitions the in-project frames name.
    ///
    /// The result refuses to overclaim. A frame matching several definitions
    /// reports `ambiguous` and lists all of them rather than picking one; a
    /// frame matching nothing reports `not_found` rather than disappearing; a
    /// frame the graph was never asked about reports `not_attempted`. Every
    /// frame read stays in the array in input order and `counts` publishes the
    /// per-outcome totals, so a caller can see what went unanswered.
    ///
    /// No source maps are read. A frame inside generated build output is
    /// reported as such, because a stale map rebinds silently to the wrong
    /// line.
    #[command(name = "trace-error")]
    TraceError {
        /// Stack trace file, or `-` to read stdin. Defaults to stdin. A
        /// relative path is resolved against the project root, matching
        /// `--diff-file`.
        #[arg(value_name = "FILE")]
        trace_file: Option<String>,
    },

    /// Auto-fix issues: remove unused exports, dependencies, and enum
    /// members; add duplicate-export rules to a fallow config file.
    ///
    /// When no fallow config exists outside a monorepo subpackage, a
    /// fresh `.fallowrc.json` is created from the same scaffolding
    /// `fallow init` would emit (framework detection, `$schema`,
    /// `entry`, etc.) and the duplicate-export rules are layered on
    /// top. Inside a monorepo subpackage the create-fallback refuses
    /// and points at the workspace root. Pass `--no-create-config` to
    /// opt out of the create-fallback (recommended for pre-commit
    /// hooks, CI bots, and `fallow watch`).
    ///
    /// Use `--dry-run` to preview source-file edits and config-file
    /// diffs without writing.
    Fix {
        /// Dry run, show what would be changed without modifying files
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation prompt (required in non-TTY environments like CI or AI agents)
        #[arg(long, alias = "force")]
        yes: bool,

        /// Refuse to create a new fallow config file when none exists.
        /// Use this from pre-commit hooks, CI bots, and `fallow watch`
        /// where silently materialising a new top-level config file would
        /// surprise the user. The duplicate-export config-add path is
        /// skipped with an explanatory message; source-file edits proceed
        /// normally.
        #[arg(long)]
        no_create_config: bool,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        /// Only fixes touching scoped files are planned and applied.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// Initialize a .fallowrc.json configuration file, AGENTS.md guide, or git
    /// pre-commit hook. Use `.fallowrc.jsonc` for editor-native JSON-with-comments
    /// support; both extensions are auto-discovered.
    ///
    /// `--hooks` scaffolds a shell-level Git pre-commit hook under
    /// `.git/hooks/` that runs fallow on changed files. The clearer hook
    /// namespace is `fallow hooks install --target git`; `init --hooks`
    /// remains as a convenience during project initialization.
    Init {
        /// Generate TOML instead of JSONC
        #[arg(long)]
        toml: bool,

        /// Scaffold a starter AGENTS.md guidance file for coding agents
        #[arg(long, conflicts_with_all = ["toml", "hooks", "branch"])]
        agents: bool,

        /// Scaffold a shell-level pre-commit git hook in `.git/hooks/` that
        /// runs fallow on changed files. Alias for
        /// `fallow hooks install --target git`.
        #[arg(long)]
        hooks: bool,

        /// Has no effect. The pre-commit hook runs `fallow audit --staged`,
        /// which compares the staged changes with HEAD. Kept so that existing
        /// install scripts still run.
        #[arg(long, requires = "hooks", hide = true)]
        branch: Option<String>,

        /// Record that this project deliberately stays unconfigured: persists a
        /// decline so the first-contact setup hint and the `setup` next-step
        /// stop appearing here. Writes no config file; idempotent
        #[arg(long, conflicts_with_all = ["toml", "agents", "hooks", "branch"])]
        decline: bool,
    },

    /// Install or remove fallow-managed Git and agent hooks.
    ///
    /// Use `fallow hooks install --target git` for a shell-level Git
    /// pre-commit hook. Use `fallow hooks install --target agent` for a
    /// Claude Code / Codex gate that blocks agent `git commit` / `git push`
    /// commands until `fallow audit` passes.
    Hooks {
        #[command(subcommand)]
        subcommand: HooksCli,
    },

    /// Wire fallow into the coding-agent harnesses used by this project in
    /// one pass (AGENTS.md task map, skill, MCP server, commit/push gate), or
    /// show and remove what was installed. `fallow init --agents` and
    /// `fallow hooks install --target agent` remain the single-piece
    /// commands underneath.
    Agent {
        #[command(subcommand)]
        subcommand: AgentCli,
    },

    /// CI helpers for PR/MR feedback envelopes.
    Ci {
        #[command(subcommand)]
        subcommand: CiCli,
    },

    /// Print the JSON Schema for fallow configuration files
    ConfigSchema,

    /// Print the JSON Schema for external plugin files
    PluginSchema,

    /// Dry-run external plugins: report what each activated and seeded
    PluginCheck,

    /// Print the JSON Schema for rule pack files
    RulePackSchema,

    /// Manage declarative rule packs (policy-as-code)
    RulePack {
        #[command(subcommand)]
        subcommand: RulePackCli,
    },

    /// Show which architecture rules apply to files before changing them.
    Guard {
        /// Files to report on (root-relative or absolute; may not exist yet)
        #[arg(required = true, num_args = 1..)]
        files: Vec<String>,
    },

    /// Show the resolved config and which config file was loaded
    ///
    /// Walks up from the project root looking for `.fallowrc.json`,
    /// `.fallowrc.jsonc`, `fallow.toml`, or `.fallow.toml`, resolves `extends`, and prints
    /// the final config as JSON. Use `--path` to print only the config
    /// file path (useful in shell scripts). The default view always exits 0:
    /// it prints the loaded config, or, on a zero-config project, the effective
    /// defaults (fully supported). `--path` exits 3 when no config file exists,
    /// since there is no path to report.
    ///
    /// Precedence is first-match-wins per directory, in the order
    /// `.fallowrc.json` > `.fallowrc.jsonc` > `fallow.toml` > `.fallow.toml`,
    /// walking up to the workspace root. `.fallowrc.json` accepts JSONC
    /// (comments and trailing commas); `.fallowrc.jsonc` is identical in
    /// behavior, the extension only signals to editors that comments are
    /// expected. If two config files coexist in one directory, fallow loads the
    /// higher-precedence one and warns on stderr naming the file it ignored.
    Config(ConfigArgs),

    /// Recommend a project-tailored config for an agent to author.
    ///
    /// Read-only. Inspects the project (frameworks, workspace layout, tooling)
    /// and emits what fallow detected, a safe proposed config, and a list of
    /// decisions split into auto (decided from detection), default (a disclosed
    /// overridable default), and taste (a genuinely subjective choice surfaced
    /// to the user as an open question). Honors `--root` and `--format`.
    Recommend,

    /// List discovered entry points, files, plugins, boundaries, workspaces, and the
    /// startup import weight.
    List {
        /// Show entry points
        #[arg(long)]
        entry_points: bool,

        /// Show all discovered files
        #[arg(long)]
        files: bool,

        /// Show active plugins
        #[arg(long)]
        plugins: bool,

        /// Show architecture boundary zones, rules, and per-zone file counts
        #[arg(long)]
        boundaries: bool,

        /// Show monorepo workspaces and any workspace-discovery diagnostics
        /// (malformed package.json, unreachable glob matches, missing
        /// tsconfig references).
        #[arg(long)]
        workspaces: bool,

        /// Show the startup import weight of each runtime entry point, in
        /// source bytes (not bundle size)
        ///
        /// For each runtime entry: the modules and source bytes that load
        /// before the entry runs, the modules behind `import()` and behind
        /// workers or forks, the packages on the startup path, and the single
        /// imports that keep the most bytes eager. Source bytes include types
        /// and comments. An import without the `type` keyword counts as eager,
        /// even when it brings in only types that TypeScript removes.
        #[arg(long)]
        entry_weight: bool,

        /// Limit the listed files, entry points and entry weights to this file or
        /// directory (default: whole project). Fallow still builds the full graph.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// Show monorepo workspaces and any workspace-discovery diagnostics.
    ///
    /// Equivalent to `fallow list --workspaces`. Use this dedicated form
    /// when introspecting only the workspace topology (other `list`
    /// sections stay hidden).
    Workspaces,

    /// Find code duplication / clones across the project
    Dupes {
        /// Detection mode: strict, mild, weak, or semantic
        /// (defaults to the value in the config file, or `mild` if unset).
        #[arg(long, help_heading = "Detection")]
        mode: Option<DupesMode>,

        /// Enable function-scoped near-miss clone detection.
        #[arg(long, help_heading = "Detection")]
        near: bool,

        /// Minimum token count for a clone
        /// (defaults to the value in the config file, or `50` if unset).
        #[arg(long, help_heading = "Detection")]
        min_tokens: Option<usize>,

        /// Minimum line count for a clone
        /// (defaults to the value in the config file, or `5` if unset).
        #[arg(long, help_heading = "Detection")]
        min_lines: Option<usize>,

        /// Minimum number of occurrences before a clone group is reported.
        /// Raise to focus on widespread copy-paste worth refactoring and skip
        /// pair-only clones.
        /// (defaults to the value in the config file, or `2` if unset).
        #[arg(long, help_heading = "Detection", value_parser = parse_min_occurrences)]
        min_occurrences: Option<usize>,

        /// Only report cross-directory duplicates
        #[arg(long, help_heading = "Detection")]
        skip_local: bool,

        /// Enable cross-language detection (strip TS type annotations for TS↔JS matching)
        #[arg(long, help_heading = "Detection")]
        cross_language: bool,

        /// Exclude module wiring from clone detection (default; covers imports,
        /// re-exports, and top-level static require bindings). Pass
        /// `--no-ignore-imports` to count it again.
        #[arg(long, help_heading = "Detection")]
        ignore_imports: bool,

        /// Count module wiring as clone candidates (opt out of the default
        /// exclusion).
        #[arg(long, help_heading = "Detection", conflicts_with = "ignore_imports")]
        no_ignore_imports: bool,

        /// Omit clone instances whose path is a symlink, or lies under a
        /// symlinked directory. A clone group with fewer than two remaining
        /// instances is not reported. Without this flag, JSON output marks
        /// these instances with `is_symlink: true`.
        #[arg(long, help_heading = "Detection")]
        ignore_symlinks: bool,

        /// Report symlinked clone instances (opt out of a config
        /// `duplicates.ignoreSymlinks: true`).
        #[arg(long, help_heading = "Detection", conflicts_with = "ignore_symlinks")]
        no_ignore_symlinks: bool,

        /// Show only the N highest-ranked clone groups. Ranking combines clone
        /// size, occurrence count, and capped directory or line spread.
        /// `clone_families[]` is rebuilt from the groups that survive, so it
        /// narrows too. `stats` keeps describing the whole corpus;
        /// `clone_groups_shown` / `clone_groups_omitted` and
        /// `clone_families_shown` / `clone_families_omitted` report the split.
        /// Refused with exit code 2 alongside `--group-by`: grouped output
        /// reports per-bucket stats over every clone group in the bucket, which
        /// a global top-N truncation would silently contradict.
        #[arg(long, help_heading = "Output")]
        top: Option<usize>,

        /// Omit the verbatim source text from each clone instance in
        /// `--format json`. The file and line/column range still address the
        /// same code, and this is most of the payload on a duplicated codebase.
        #[arg(long, help_heading = "Output")]
        no_fragments: bool,

        /// Trace the clones at a location (`FILE:LINE`, for example `src/utils.ts:42`) or the clone group of a fingerprint (`dup:<fingerprint>`, for example `dup:f42424e8`)
        #[arg(long, help_heading = "Output")]
        #[arg(value_name = "FILE:LINE|dup:<fingerprint>")]
        trace: Option<String>,

        /// Fail if duplication exceeds this percentage (0 = no limit) (defaults to the value in the config file, or `0` if unset).
        #[arg(long, help_heading = "Gates")]
        threshold: Option<f64>,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// Analyze function complexity (cyclomatic + cognitive)
    ///
    /// By default, shows all existing sections: health score, complexity findings,
    /// file scores, hotspots, and refactoring targets. When any section flag is
    /// specified, only those sections are shown.
    Health {
        /// Maximum cyclomatic complexity threshold (overrides config)
        #[arg(long, help_heading = "Detection")]
        max_cyclomatic: Option<u16>,

        /// Maximum cognitive complexity threshold (overrides config)
        #[arg(long, help_heading = "Detection")]
        max_cognitive: Option<u16>,

        /// Maximum CRAP score threshold (overrides config, default 30.0).
        /// Functions meeting or exceeding this score are reported alongside
        /// complexity findings. Pair with `--coverage` for accurate scoring.
        #[arg(long, help_heading = "Detection")]
        max_crap: Option<f64>,

        /// Maximum physical lines per file (overrides config `health.maxFileLines`,
        /// default 0 = off). A longer file is reported as a complexity finding
        /// with `exceeded: "file_lines"`.
        #[arg(long, value_name = "LINES", help_heading = "Detection")]
        max_file_lines: Option<u32>,

        /// Show only complexity findings (functions exceeding thresholds).
        /// By default all sections are shown; use this to select only complexity.
        #[arg(long, help_heading = "Sections")]
        complexity: bool,

        /// Include the per-decision-point complexity breakdown (`contributions[]`)
        /// on each complexity finding in `--format json` output. Each entry names
        /// the construct (if, else-if, loop, boolean operator, ...) and its
        /// cyclomatic/cognitive weight, so a consumer can explain WHY a function
        /// scored high. Used by the VS Code inline editor breakdown. Off by
        /// default to keep CI/default output lean.
        #[arg(long, help_heading = "Output")]
        complexity_breakdown: bool,

        /// Show only the N most complex functions
        #[arg(long, help_heading = "Output")]
        top: Option<usize>,

        /// Sort by: cyclomatic (default), cognitive, lines, or severity
        #[arg(long, default_value = "cyclomatic", help_heading = "Output")]
        sort: SortBy,

        /// Show only per-file health scores (fan-in, fan-out, dead code ratio, maintainability index).
        /// Requires full analysis pipeline (graph + dead code detection).
        /// Sorted by risk-aware triage concern: lower MI and higher CRAP risk first.
        /// --sort and --baseline apply to complexity findings only, not file scores.
        #[arg(long, help_heading = "Sections")]
        file_scores: bool,

        /// Show only static test coverage gaps: runtime files and exports with no
        /// dependency path from any discovered test root. Requires full analysis pipeline.
        #[arg(long, help_heading = "Sections")]
        coverage_gaps: bool,

        /// Show only hotspots: files that are both complex and frequently changing.
        /// Combines git churn history with complexity data. Requires a git repository.
        #[arg(long, help_heading = "Sections")]
        hotspots: bool,

        /// Attach ownership signals to hotspot entries: bus factor, contributor
        /// count, declared CODEOWNERS owner, and ownership drift. Implies
        /// `--hotspots`. Requires a git repository.
        #[arg(long, help_heading = "Sections")]
        ownership: bool,

        /// Privacy mode for author emails emitted with `--ownership`.
        /// Defaults to `handle` (local-part only). Use `raw` for OSS repos
        /// where authors are public, or `anonymized` to emit non-reversible
        /// pseudonyms in regulated environments. Implies `--ownership`.
        #[arg(long, value_name = "MODE", value_enum, help_heading = "Sections")]
        ownership_emails: Option<EmailModeArg>,

        /// Show only refactoring targets: ranked recommendations based on complexity,
        /// coupling, churn, and dead code signals. Requires full analysis pipeline.
        #[arg(long, help_heading = "Sections")]
        targets: bool,

        /// Show advisory project-local public-signature type coupling. Requires
        /// type-aware analysis and does not change the health score.
        #[arg(long, help_heading = "Sections")]
        type_coupling: bool,

        /// Add structural CSS analytics: specificity hotspots, !important density,
        /// over-complex selectors, deep nesting, and conservative cleanup
        /// candidates. Standard CSS is parsed structurally; preprocessor sources
        /// are scanned only where fallow can avoid expanding Sass/Less semantics.
        #[arg(long, help_heading = "Sections")]
        css: bool,

        /// Filter refactoring targets by effort level (low, medium, high).
        /// Implies --targets.
        #[arg(long, value_enum, help_heading = "Sections")]
        effort: Option<EffortFilter>,

        /// Show only the project health score (0–100) with letter grade (A/B/C/D/F).
        /// The score is included by default when no section flags are set.
        #[arg(long, help_heading = "Sections")]
        score: bool,

        /// Fail if the health score is below this threshold (0-100).
        /// Implies --score. The authoritative CI quality gate: when set,
        /// complexity findings become informational and the exit code is
        /// driven solely by the score (so --min-score 0 always exits 0).
        /// Composes with --min-severity (fails if either gate trips). Plain
        /// `fallow health` (no gate flag) stays advisory and exits 1 on any
        /// finding; for a gate on newly-introduced complexity use
        /// `fallow audit --gate new-only`.
        #[arg(long, value_name = "N", help_heading = "Gates")]
        min_score: Option<f64>,

        /// Only exit with error for findings at or above this severity.
        /// Use --min-severity critical to ignore moderate/high findings in CI.
        /// Composes with --min-score (the run fails if either gate trips).
        /// The complexity-* rules apply first: a finding whose rule is warn
        /// never fails the run, whatever its severity.
        #[arg(long, value_name = "LEVEL", value_enum, help_heading = "Gates")]
        min_severity: Option<HealthSeverityCli>,

        /// Print the score and findings but never fail CI (always exit 0).
        /// Advisory mode for surfacing health in logs without blocking.
        /// Mutually exclusive with --min-score and --min-severity.
        #[arg(long, help_heading = "Gates")]
        report_only: bool,

        /// Git history window for hotspot analysis (default: 6m).
        /// Accepts durations (6m, 90d, 1y, 2w) or ISO dates (2025-06-01).
        #[arg(long, value_name = "DURATION", help_heading = "Detection")]
        since: Option<String>,

        /// Minimum number of commits for a file to be included in hotspot ranking (default: 3)
        #[arg(long, value_name = "N", help_heading = "Detection")]
        min_commits: Option<u32>,

        /// Save a vital signs snapshot for trend tracking.
        /// Defaults to `.fallow/snapshots/{timestamp}.json` if no path is given.
        /// A given path must resolve inside the project root, its Git work tree, the CI workspace or a temp directory.
        /// Forces file-scores, hotspot, and score computation for complete metrics.
        #[expect(
            clippy::option_option,
            reason = "clap pattern: None=not passed, Some(None)=flag only, Some(Some(path))=with value"
        )]
        #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = "", help_heading = "Output")]
        save_snapshot: Option<Option<String>>,

        /// Compare current metrics against the most recent saved snapshot.
        /// Reads from `.fallow/snapshots/` and shows per-metric deltas with
        /// directional indicators. Implies --score.
        #[arg(long, help_heading = "Sections")]
        trend: bool,

        /// Compare current metrics against this snapshot file instead of the
        /// newest file in `.fallow/snapshots/`. Use it to restore a baseline
        /// from external storage in CI. With --group-by, groups are compared
        /// by key when the snapshot holds the same grouping. Implies --trend.
        #[arg(long, value_name = "PATH", help_heading = "Sections")]
        trend_from: Option<PathBuf>,

        /// Path to coverage data for measured per-function CRAP scores. Accepts an
        /// Istanbul coverage map JSON file (coverage-final.json, from
        /// `jest --coverage`, `vitest run --coverage`, c8 or nyc), a directory
        /// containing coverage-final.json, a raw V8 coverage directory
        /// (`NODE_V8_COVERAGE=<dir> node --test`), or a single V8 coverage JSON
        /// file. Transpiled V8 scripts (tsx, bundles) map back to their source
        /// files through the source map that Node records in the dump. A
        /// script that differs from the file on disk and has no source map
        /// keeps the estimate. Use --coverage-root when
        /// the data was generated in a different environment (CI runner,
        /// Docker). Affects CRAP scores only, not --coverage-gaps. Also
        /// configurable via FALLOW_COVERAGE env var.
        #[arg(long, value_name = "PATH", help_heading = "Coverage")]
        coverage: Option<PathBuf>,

        /// Absolute prefix to strip from file paths in coverage data before
        /// prepending the project root. Use when coverage was generated in a
        /// different environment (CI runner, Docker). Example: if coverage paths
        /// start with /home/runner/work/myapp and the project root is ./,
        /// pass --coverage-root /home/runner/work/myapp.
        #[arg(long, value_name = "PATH", help_heading = "Coverage")]
        coverage_root: Option<PathBuf>,

        /// File or directory containing runtime coverage input. Accepts a
        /// V8 coverage directory, a single V8 JSON file, or a single
        /// Istanbul coverage map JSON file (commonly coverage-final.json).
        /// A single local capture is free. Continuous or multi-capture
        /// monitoring needs a license (see `fallow license`).
        #[arg(long, value_name = "PATH", help_heading = "Coverage")]
        runtime_coverage: Option<PathBuf>,

        /// Saved `fallow mutation --format json` report (for example
        /// `.fallow/mutation/last-report.json` or the `-o` file of a run).
        /// Adds the mutation score of each complexity finding next to its
        /// CRAP score. The score never changes CRAP or the health score. A
        /// function whose file changed since the run gets no score. Reads
        /// the file only; it runs no test.
        #[arg(long, value_name = "FILE", help_heading = "Coverage")]
        mutation_report: Option<PathBuf>,

        /// Threshold for hot-path classification
        #[arg(long, default_value_t = 100, help_heading = "Coverage")]
        min_invocations_hot: u64,

        /// Minimum total trace volume before the runtime coverage add-on gives
        /// high-confidence `safe_to_delete` / `review_required` verdicts. Below
        /// this, the add-on caps confidence at `medium` to protect against
        /// overconfident verdicts on new or low-traffic services. Omit to use
        /// the add-on default (5000).
        #[arg(long, value_name = "N", help_heading = "Coverage")]
        min_observation_volume: Option<u32>,

        /// Fraction of total trace count below which an invoked function is
        /// classified as `low_traffic` rather than `active`. Expressed as a
        /// decimal (e.g. `0.001` for 0.1%). Omit to use the runtime coverage
        /// add-on default (0.001).
        #[arg(long, value_name = "RATIO", help_heading = "Coverage")]
        low_traffic_threshold: Option<f64>,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// Detect feature flag patterns in the codebase
    ///
    /// Identifies environment variable flags (process.env.FEATURE_*),
    /// SDK calls from common providers, and config object patterns (opt-in).
    /// Reports flag locations, detection confidence, and cross-reference
    /// with dead code findings.
    Flags {
        /// Add a retirement report: one row per flag, with the reasons the
        /// flag can be retired. Advisory only; nothing is removed.
        #[arg(long, help_heading = "Sections")]
        retirement: bool,

        /// Keep only retirement rows with this reason (repeatable)
        #[arg(
            long = "reason",
            value_name = "CODE",
            requires = "retirement",
            help_heading = "Sections"
        )]
        reasons: Vec<flags::RetirementReasonArg>,

        /// Order of the retirement rows
        #[arg(
            long,
            value_name = "KEY",
            requires = "retirement",
            default_value = "age",
            help_heading = "Sections"
        )]
        sort: flags::RetirementSortArg,

        /// How to measure flag age: blame (lower bound), pickaxe (first
        /// commit with the name, slower) or off
        #[arg(
            long,
            value_name = "MODE",
            requires = "retirement",
            default_value = "blame",
            help_heading = "Detection"
        )]
        flag_age: flags::FlagAgeArg,

        /// Keep only retirement rows at least this many days old
        #[arg(
            long,
            value_name = "DAYS",
            requires = "retirement",
            help_heading = "Sections"
        )]
        min_age: Option<u64>,

        /// Vendor flag export (JSON, read offline) that adds the
        /// fully-rolled-out, archived-in-vendor, missing-in-vendor and
        /// vendor-only reasons
        #[arg(
            long,
            value_name = "FILE",
            requires = "retirement",
            help_heading = "Detection"
        )]
        flag_state: Option<std::path::PathBuf>,

        /// Show only the top N flags
        #[arg(long, help_heading = "Output")]
        top: Option<usize>,

        /// Exit with code 1 when a flag in scope is older than this many
        /// days. Opt-in; needs a flag age
        #[arg(
            long,
            value_name = "DAYS",
            requires = "retirement",
            help_heading = "Gates"
        )]
        max_flag_age: Option<u64>,
    },

    /// List active fallow-ignore suppression markers (read-only inventory)
    ///
    /// Shows every `fallow-ignore-next-line` and `fallow-ignore-file` marker
    /// present in analyzed files, grouped per file with line, kind, level,
    /// and reason, plus project totals and a stale cross-reference against
    /// this run's stale-suppression findings. A governance surface, not a
    /// detector: always exits 0. Honors `--root`, `--format {human,json}`,
    /// `--workspace`, `--changed-workspaces`, `--changed-since`, and
    /// `--quiet`.
    Suppressions {
        /// Only list suppressions in the specified files. Accepts multiple values.
        #[arg(long, value_name = "PATH")]
        file: Vec<std::path::PathBuf>,
    },

    /// Explain one fallow issue type without running an analysis.
    ///
    /// Prints the rule rationale, a worked example, fix guidance, and the
    /// relevant docs URL. Accepts values like `unused-export`,
    /// `fallow/unused-export`, `unused exports`, and `code duplication`.
    Explain {
        /// Issue type, issue label, or rule id to explain
        #[arg(required = true, num_args = 1.., value_name = "ISSUE_TYPE")]
        issue_type: Vec<String>,
    },

    /// Audit changed files for dead code, complexity, duplication, and styling.
    ///
    /// Purpose-built for reviewing AI-generated code and PR quality gates.
    /// Combines dead-code + complexity + duplication + styling scoped to
    /// changed files and returns a verdict (pass/warn/fail).
    ///
    /// `fallow audit` answers "will CI block this?": it gates (exit 1 on a fail verdict). The
    /// `review` alias plus `--brief` answer "where do I look?": the same analysis rendered as a
    /// deterministic orientation brief that always exits 0, so a reviewer or agent can read it
    /// regardless of the verdict. `--format` is orthogonal to `--brief`. When
    /// `--changed-since`/`--base` is unset, the base is the git merge-base against the branch's
    /// upstream or the remote default (`origin/HEAD`, `origin/main`, `origin/master`); set
    /// `FALLOW_AUDIT_BASE` to pin it. By default, only findings introduced by the changeset
    /// affect the verdict; inherited findings are reported with new-vs-inherited attribution
    /// and individual JSON findings include `introduced: true/false`. The default gate also
    /// reports an unused file or unused export outside the changed files that the base does not
    /// report: the change removed its last use, or a config change made it unreachable. Use
    /// `--gate all` or `[audit] gate = "all"` to fail on every finding in changed files without
    /// running the extra base-snapshot attribution pass.
    ///
    /// The global --baseline / --save-baseline flags are rejected on audit.
    /// Use --dead-code-baseline, --health-baseline, and --dupes-baseline
    /// (or their config equivalents) because each sub-analysis uses a
    /// different baseline format.
    #[command(visible_alias = "review")]
    Audit {
        /// Run dead-code analysis in production mode for this audit.
        #[arg(long = "production-dead-code", help_heading = "Scope")]
        production_dead_code: bool,

        /// Run health analysis in production mode for this audit.
        #[arg(long = "production-health", help_heading = "Scope")]
        production_health: bool,

        /// Run duplication analysis in production mode for this audit.
        #[arg(long = "production-dupes", help_heading = "Scope")]
        production_dupes: bool,

        /// Compare dead-code issues against a saved baseline
        /// (produced by `fallow dead-code --save-baseline`).
        #[arg(long, help_heading = "Baselines")]
        dead_code_baseline: Option<PathBuf>,

        /// Compare health findings against a saved baseline
        /// (produced by `fallow health --save-baseline`).
        #[arg(long, help_heading = "Baselines")]
        health_baseline: Option<PathBuf>,

        /// Compare duplication clone groups against a saved baseline
        /// (produced by `fallow dupes --save-baseline`).
        #[arg(long, help_heading = "Baselines")]
        dupes_baseline: Option<PathBuf>,

        /// Maximum CRAP score threshold (overrides config, default 30.0).
        /// Functions meeting or exceeding this score cause audit to fail.
        /// Pair with `--coverage` for accurate scoring.
        #[arg(long, help_heading = "Coverage")]
        max_crap: Option<f64>,

        /// Path to Istanbul coverage data (coverage-final.json) or raw V8
        /// coverage (a `NODE_V8_COVERAGE` directory or one V8 JSON file) for
        /// accurate per-function CRAP scores in the health sub-analysis. Also
        /// configurable via FALLOW_COVERAGE or health.coverage.
        #[arg(long, value_name = "PATH", help_heading = "Coverage")]
        coverage: Option<PathBuf>,

        /// Absolute prefix to strip from coverage data paths before CRAP matching.
        /// Use when coverage was generated under a different checkout root in CI or Docker.
        /// Also configurable via FALLOW_COVERAGE_ROOT or health.coverageRoot.
        #[arg(long, value_name = "PATH", help_heading = "Coverage")]
        coverage_root: Option<PathBuf>,

        /// Run only these sub-analyses (comma-separated: dead-code,
        /// architecture, dupes, health), with the same values as bare
        /// `fallow --only`. Replaces the `audit.deadCode`, `audit.health` and
        /// `audit.dupes` selection. The review brief (`--brief`, `fallow
        /// review`) keeps every sub-analysis.
        #[arg(
            long,
            value_delimiter = ',',
            conflicts_with = "skip",
            help_heading = "Scope"
        )]
        only: Vec<AnalysisKind>,

        /// Skip these sub-analyses (comma-separated: dead-code, architecture,
        /// dupes, health), with the same values as bare `fallow --skip`, on
        /// top of the `audit.*` selection.
        #[arg(long, value_delimiter = ',', help_heading = "Scope")]
        skip: Vec<AnalysisKind>,

        /// Disable styling analytics in audit.
        #[arg(long = "no-css", help_heading = "Scope")]
        no_css: bool,

        /// Enable deep CSS analysis for audit explicitly: project-wide styling
        /// reachability, narrowed back to changed anchors. Deep CSS is on by
        /// default; use this to override `audit.cssDeep = false`.
        #[arg(long, help_heading = "Scope")]
        css_deep: bool,

        /// Disable deep CSS analysis while keeping local styling analytics on.
        #[arg(long = "no-css-deep", help_heading = "Scope")]
        no_css_deep: bool,

        /// Which findings affect the audit verdict.
        ///
        /// new-only (default): fail only on findings introduced by the current
        /// changeset. all: fail on every finding in changed files and skip
        /// base-snapshot attribution.
        #[arg(long, value_enum, help_heading = "Gates")]
        gate: Option<AuditGateArg>,

        /// How a worsened complexity finding affects the audit verdict.
        ///
        /// A worsened finding was already in the base snapshot, and a measured
        /// value is higher now. Any growth of cyclomatic, cognitive or CRAP
        /// counts. The line count must grow by at least 5 lines. It includes
        /// comments and blank lines in the function.
        /// warn (default): the verdict is at least warn, and the run does not
        /// fail. error: a worsened finding counts like an introduced finding.
        /// off: only growth of an exceeded metric stays worsened (warn).
        /// Overrides `audit.worsened` in config. Has no effect with
        /// `--gate all`, which counts every finding.
        #[arg(long, value_enum, help_heading = "Gates")]
        worsened: Option<AuditWorsenedArg>,

        /// Runtime coverage input. Accepts a V8 directory, a single V8 JSON
        /// file, or an Istanbul coverage map JSON. Runs the runtime coverage
        /// add-on inside the audit, so the `hot-path-touched` verdict shows
        /// next to the dead-code and complexity findings without a second
        /// `fallow health` run in CI. The verdict is informational and does
        /// not change the exit code. A single local capture is free.
        /// Continuous or multi-capture monitoring needs a license (see
        /// `fallow license`).
        #[arg(long, value_name = "PATH", help_heading = "Coverage")]
        runtime_coverage: Option<PathBuf>,

        /// Threshold for hot-path classification, passed to the runtime
        /// coverage add-on when `--runtime-coverage` is set.
        #[arg(long, default_value_t = 100, help_heading = "Coverage")]
        min_invocations_hot: u64,

        /// Saved `fallow mutation --format json` report (for example
        /// `.fallow/mutation/last-report.json`). The review brief counts the
        /// surviving mutants on the added lines of each changed file in the
        /// `mutation` component of the focus score. A file whose source
        /// changed since the run adds nothing. Reads the file only; it runs no
        /// test. Only the review brief reads it.
        #[arg(long, value_name = "FILE", help_heading = "Coverage")]
        mutation_report: Option<PathBuf>,

        #[command(flatten)]
        gate_run: AuditGateRunArgs,

        /// Render the deterministic review brief instead of the gating audit
        /// report. The brief answers "where do I look?" rather than "will CI
        /// block this?", runs the same analysis, and always exits 0 (the
        /// verdict is carried informationally). Implied by `fallow review`.
        /// Orthogonal to `--format`.
        #[arg(long, help_heading = "Review")]
        brief: bool,

        /// Cap on the number of consequential structural decisions surfaced in
        /// the review brief's decision surface (the working-memory limit).
        /// Default 4; clamped to the range 3 to 5. Only consulted on the
        /// brief path.
        #[arg(
            long,
            value_name = "N",
            default_value_t = audit_decision_surface::DEFAULT_DECISION_CAP,
            help_heading = "Review",
        )]
        max_decisions: usize,

        /// Emit the walkthrough guide for an agent: the current digest
        /// (brief + decision surface), the review direction, the JSON schema the
        /// agent must return, and a deterministic graph-snapshot hash pinned into
        /// the digest. The digest is built from the graph only (PR prose is never
        /// folded in, so it is injection-resistant). Implies the brief; always
        /// exits 0. A thin agent skill calls this to fetch the current guide,
        /// produces judgment JSON, then reopens with `--walkthrough-file`.
        #[arg(long, conflicts_with_all = ["walkthrough_file", "walkthrough"], help_heading = "Review")]
        walkthrough_guide: bool,

        /// Read the judgment JSON of an agent and check it against the
        /// current graph. Rejects any judgment whose `signal_id` fallow did not emit;
        /// refuses the whole payload as stale when the
        /// echoed graph-snapshot hash no longer matches (the tree moved); rejects
        /// an `action` outside `block`, `address`, `consider`, `fyi`
        /// (`invalid-action`). The verifier is the graph, not a second model.
        /// Implies the brief; always exits 0. The agent's free-text framing and
        /// action label are fenced as non-deterministic and never gate or
        /// auto-post.
        #[arg(long, value_name = "PATH", help_heading = "Review")]
        walkthrough_file: Option<PathBuf>,

        /// Render the existing walkthrough guide as a staged terminal tour for a person
        /// (Stage 1 load-bearing / Stage 2 mechanical), or markdown with
        /// `--format markdown`. Implies the brief; always exits 0.
        /// `--format json --walkthrough` emits the same agent-contract JSON as
        /// `--walkthrough-guide`.
        #[arg(long, conflicts_with_all = ["walkthrough_guide", "walkthrough_file"], help_heading = "Review")]
        walkthrough: bool,

        /// Record one or more changed files as viewed in the local walkthrough
        /// viewed-state ledger (`.fallow/walkthrough-state.json`), then render the
        /// tour. Files already viewed (and still current) collapse into the
        /// Cleared panel. Repeatable. Stale marks (the tree moved) are ignored on
        /// render but never deleted. Only consulted on the `--walkthrough` path.
        #[arg(long, value_name = "PATH", help_heading = "Review")]
        mark_viewed: Vec<PathBuf>,

        /// Expand the Cleared panel in the human/markdown walkthrough tour: list
        /// each de-prioritized and already-viewed file instead of the collapsed
        /// one-line summary. Only consulted on the `--walkthrough` path.
        #[arg(long, help_heading = "Review")]
        show_cleared: bool,

        /// Expand the de-prioritized units in the review brief's weighted
        /// focus map ("show me what you de-prioritized"). The `deprioritized`
        /// escape-hatch list is always present in `--format json` regardless; this
        /// flag only re-expands the collapse-by-default human focus render. Only
        /// consulted on the `--walkthrough` path.
        #[arg(long, help_heading = "Review")]
        show_deprioritized: bool,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// Maintain reusable audit base-snapshot caches.
    AuditCache {
        #[command(subcommand)]
        subcommand: AuditCacheCli,
    },

    /// Maintain the committed baseline files that the config points at.
    Baselines {
        #[command(subcommand)]
        subcommand: BaselinesCli,
    },

    /// Surface the consequential structural decisions a change embeds (the apex
    /// of the review brief), each framed as a judgment question with the routed
    /// expert to ask.
    ///
    /// A ranked, capped (3 to 5), signal_id-anchored set of decisions in three
    /// categories: coupling/boundary, exports-aware public-API/contract and
    /// dependency. Runs the same changed-code
    /// analysis as `fallow review` but emits only the decisions, separable and
    /// cheap. Coupling and public-API decisions are suppressible with
    /// `// fallow-ignore`; a dependency decision anchors on `package.json` and
    /// has no suppress action. Never gates: exits 0 after a successful run, and
    /// exits 2 on invalid input (for example the global `--baseline` or
    /// `--save-baseline` flag) like every other command. Use `--base` /
    /// `--changed-since` to pick the comparison point, exactly like `fallow audit`.
    DecisionSurface {
        /// Cap on the number of surfaced decisions (the working-memory limit).
        /// Default 4; clamped to the range 3 to 5.
        #[arg(
            long,
            value_name = "N",
            default_value_t = audit_decision_surface::DEFAULT_DECISION_CAP
        )]
        max_decisions: usize,
    },

    /// Show what fallow has done for you: how many issues it is surfacing, the
    /// trend since the last recorded run, and how many commits it contained at
    /// the pre-commit gate.
    ///
    /// Local-only and opt-in: enable per project with `fallow impact enable`, or
    /// turn it on everywhere with `fallow impact default on`, then let your
    /// `fallow audit` / pre-commit gate runs build history. History is stored in
    /// your user config dir (never written into the repo) and forced off in CI.
    /// Impact never uploads anything and never affects exit codes.
    Impact {
        #[command(subcommand)]
        subcommand: Option<ImpactCli>,
        /// Aggregate every tracked project into one cross-repo roll-up
        /// ("what has fallow done for me across all my repos"). Reads the
        /// user config dir; ignores `--root`. Cannot combine with a subcommand.
        #[arg(long)]
        all: bool,
        /// Row ordering for `--all` (default: most recently recorded first).
        #[arg(long, value_enum, default_value_t = ImpactSortCli::Recent)]
        sort: ImpactSortCli,
        /// Cap the number of `--all` rows printed (grand totals still reflect
        /// every tracked project).
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Surface local security candidates for downstream agent verification (opt-in).
    ///
    /// Ships three complementary surfaces. (1) The graph-structural
    /// `client-server-leak` rule: a `"use client"` file that transitively imports
    /// a module reading a non-public env secret through `process.env` or
    /// `import.meta.env`. (2) The data-driven
    /// `tainted-sink` catalogue: syntactic sink sites matched against a CWE
    /// catalogue (`security_matchers.toml`) spanning categories such as
    /// dangerous-html, template-escape-bypass, command-injection, code-injection,
    /// dynamic-regex, redos-regex, resource-amplification, dynamic-module-load,
    /// sql-injection, ssrf, path-traversal, header-injection, open-redirect,
    /// cleartext-transport, electron-unsafe-webpreferences,
    /// world-writable-permission, insecure-temp-file,
    /// mysql-multiple-statements, mass-assignment, weak-crypto,
    /// deprecated-cipher, insecure-randomness,
    /// unsafe-buffer-alloc, unsafe-deserialization, prototype-pollution,
    /// zip-slip, nosql-injection, ssti, xxe, xpath-injection, and
    /// webview-injection. (3) `hardcoded-secret`,
    /// an include-required
    /// category for provider-prefix literals and high-entropy literals assigned
    /// to secret-shaped identifiers. It never runs from raw entropy alone. All
    /// findings are candidates for verification, not verified vulnerabilities.
    /// This command is the only
    /// surface for security findings; they never appear under bare `fallow` or
    /// the `audit` gate. Build-config and test files are excluded, and public
    /// env prefixes such as `NEXT_PUBLIC_` and `VITE_` are treated as public.
    /// Honors
    /// `--root`, `--format {human,json,sarif}`, `--changed-since`, `--file`, `--gate`, `--diff-file`,
    /// `--diff-stdin`, `--workspace`, `--changed-workspaces`, `--ci`,
    /// `--fail-on-issues`, `--sarif-file`, `--summary`, `--explain`, and `--surface`.
    Security {
        #[command(subcommand)]
        subcommand: Option<SecuritySubcommand>,
        /// Runtime coverage input. Accepts a V8 directory, a single V8 JSON
        /// file, or an Istanbul coverage map JSON. When set, `fallow security`
        /// adds production runtime state to tainted-sink candidates and uses
        /// that state as an extra ranking signal. A single local capture is
        /// free. Continuous or multi-capture monitoring needs a license (see
        /// `fallow license`).
        #[arg(long, value_name = "PATH")]
        runtime_coverage: Option<PathBuf>,
        /// Threshold for hot-path classification, passed to the runtime
        /// coverage add-on when `--runtime-coverage` is set.
        #[arg(long, default_value_t = 100)]
        min_invocations_hot: u64,
        /// Only report security candidates in or reachable from the specified files.
        /// The full project graph is still built, but output is scoped to matching
        /// finding anchors or trace hops. Accepts multiple values.
        #[arg(long, value_name = "PATH")]
        file: Vec<std::path::PathBuf>,
        /// Opt-in regression gate: fail (exit 8) only when the change introduces a
        /// NEW security-sink candidate in the changed lines, not on the whole
        /// candidate backlog. Requires a diff source: `--changed-since <ref>`,
        /// `--diff-file <path>`, or `--diff-stdin`. There is deliberately no `all`
        /// mode (gating on the full backlog is the anti-feature this gate avoids).
        #[arg(long, value_name = "MODE")]
        gate: Option<security::SecurityGateArg>,
        /// Include the agent-facing attack-surface inventory in JSON output.
        #[arg(long)]
        surface: bool,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// Render a saved `--format json` results file in another format without
    /// re-running analysis (analyze once, then render every CI surface from
    /// the same file). Supports GitHub annotations/summary, CodeClimate,
    /// SARIF, markdown, and GitHub/GitLab PR-comment and review formats.
    Report {
        /// Path to a fallow JSON results file produced by `--format json`
        /// (dead-code, architecture, dupes, health, audit, security, or bare
        /// combined).
        #[arg(long, value_name = "PATH")]
        from: PathBuf,
        /// A saved `fallow mutation --format json` report, or the JSON error
        /// envelope of a failed run, to render with the results. Applies to
        /// the pr-comment, review, github-summary, github-annotations and
        /// codeclimate formats only. The paths in the report are relative to
        /// the root of the mutation run, which must be the `--root` of this
        /// call.
        #[arg(long, value_name = "PATH")]
        mutation_report: Option<PathBuf>,
        /// Why mutation testing did not run in this job, so the render states
        /// it. Applies to the same formats as `--mutation-report`.
        #[arg(long, value_name = "REASON", conflicts_with = "mutation_report")]
        mutation_not_run: Option<MutationNotRunArg>,
    },
    /// Dump fallow's capability manifest (CLI commands and flags, issue types, MCP tools, framework plugins, env vars) as machine-readable JSON for agent introspection. Always JSON, regardless of --format
    Schema(crate::schema::SchemaArgs),

    /// Print or vendor CI integration templates.
    ///
    /// Use `fallow ci-template gitlab` to print the GitLab CI template, or
    /// `fallow ci-template gitlab --vendor` to write the template plus the
    /// bash helper files that enable MR comments without downloading from
    /// raw.githubusercontent.com at pipeline runtime.
    CiTemplate {
        #[command(subcommand)]
        subcommand: CiTemplateCli,
    },

    /// Migrate configuration from knip, jscpd, or stylelint to fallow
    Migrate {
        /// Generate `fallow.toml` instead of JSONC
        #[arg(long, conflicts_with = "jsonc")]
        toml: bool,

        /// Write JSONC content to `.fallowrc.jsonc` instead of `.fallowrc.json`. The
        /// generated content is the same JSONC (with `//` comments) either way; the
        /// `.jsonc` extension lets editors auto-detect JSON-with-comments syntax
        /// highlighting and silences linters that flag comments in `.json`. Without
        /// `--jsonc` or `--toml`, fallow auto-mirrors the source extension: a
        /// `knip.jsonc` migration writes `.fallowrc.jsonc`, a `knip.json` migration
        /// writes `.fallowrc.json`.
        #[arg(long)]
        jsonc: bool,

        /// Only preview the generated config without writing
        #[arg(long)]
        dry_run: bool,

        /// Path to source config file (auto-detect if not specified)
        #[arg(long, value_name = "PATH")]
        from: Option<PathBuf>,
    },

    /// Manage the license for continuous/cloud runtime monitoring.
    ///
    /// Verification is offline against an Ed25519 public key compiled into
    /// the binary. The license file lives at `~/.fallow/license.jwt` (or
    /// `$FALLOW_LICENSE_PATH`); `$FALLOW_LICENSE` env var takes precedence
    /// and is the recommended path for shared CI runners.
    License {
        #[command(subcommand)]
        subcommand: LicenseCli,
    },

    /// Manage opt-in product telemetry.
    ///
    /// Telemetry is off by default. It never collects repository names, paths,
    /// package names, source code, config values, raw errors, or raw agent
    /// detection evidence. Use `fallow telemetry inspect --example` to see the
    /// documented payload shape, or prefix a real command with
    /// `FALLOW_TELEMETRY=inspect` to print the exact payload without sending.
    Telemetry {
        #[command(subcommand)]
        subcommand: TelemetryCli,
    },

    /// Runtime coverage workflow.
    ///
    /// `setup` is the resumable first-run flow: license check, runtime
    /// coverage add-on install, coverage recipe, then analysis.
    Coverage {
        #[command(subcommand)]
        subcommand: CoverageCli,
    },

    /// Install or remove a Claude Code PreToolUse hook that gates
    /// `git commit` / `git push` on `fallow audit`, so the agent cleans
    /// findings before the command runs.
    ///
    /// Deprecated: use `fallow agent install` (one pass for every harness)
    /// or `fallow hooks install --target agent` (the gate alone). This
    /// command keeps working throughout fallow 3 and is removed in the next
    /// major. It writes into `.claude/settings.json` +
    /// `.claude/hooks/fallow-gate.sh`, or for Codex into `.codex/hooks.json` +
    /// `.codex/hooks/fallow-gate.sh` plus an `AGENTS.md` routing block. For a
    /// shell-level Git pre-commit hook in
    /// `.git/hooks/`, see `fallow hooks install --target git` instead.
    SetupHooks {
        /// Target a specific agent surface (default: auto-detect).
        #[arg(long, value_enum)]
        agent: Option<setup_hooks::HookAgentArg>,

        /// Print what would be written or removed without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Overwrite a user-edited hook script, invalid settings.json, or
        /// remove a user-edited script during uninstall.
        #[arg(long)]
        force: bool,

        /// Write to the user's home directory instead of the project root.
        #[arg(long)]
        user: bool,

        /// Append `.claude/` to the project's `.gitignore`.
        #[arg(long)]
        gitignore_claude: bool,

        /// Remove the fallow-gate handler, hook script, and AGENTS.md
        /// managed block instead of installing them. Idempotent: reports
        /// "unchanged" when nothing to remove.
        #[arg(long)]
        uninstall: bool,
    },

    /// Generate an interactive HTML map of the codebase
    Viz {
        /// Output file path (default: fallow-viz.html in project root)
        #[arg(long = "out", value_name = "PATH")]
        output: Option<PathBuf>,

        /// Do not open the output file in the browser
        #[arg(long)]
        no_open: bool,

        /// Visualization output format
        #[arg(long = "viz-format", default_value = "html")]
        viz_format: viz::VizFormat,
    },
}

#[derive(clap::Subcommand)]
pub enum BaselinesCli {
    /// Remove the baseline entries that match no current finding.
    ///
    /// Reads the files that `audit.deadCodeBaseline`, `audit.healthBaseline`
    /// and `audit.dupesBaseline` name, runs one analysis over the whole project,
    /// in production mode with --production or config, and removes each entry
    /// that `--baseline` would no longer match. Prune never adds an entry. To
    /// record new findings on purpose, use `fallow <kind> --save-baseline`.
    ///
    /// Exit codes: 0 when the files are pruned or have nothing to prune, 1 with
    /// `--check` when an entry can be pruned, and 2 when a file cannot be read
    /// or pruned. A file with an older key form or another mode (`--production`,
    /// `--include-entry-exports`) is skipped with the command to use.
    Prune {
        /// Report the entries that a prune would remove, and write nothing.
        #[arg(long)]
        check: bool,

        /// The dead-code baseline to prune. Overrides `audit.deadCodeBaseline`.
        #[arg(long, value_name = "PATH")]
        dead_code_baseline: Option<PathBuf>,

        /// The health baseline to prune. Overrides `audit.healthBaseline`.
        #[arg(long, value_name = "PATH")]
        health_baseline: Option<PathBuf>,

        /// The duplication baseline to prune. Overrides `audit.dupesBaseline`.
        #[arg(long, value_name = "PATH")]
        dupes_baseline: Option<PathBuf>,

        /// The coverage input for CRAP scores, as for `fallow health
        /// --coverage`. Use the input that the health baseline was saved with.
        #[arg(long, value_name = "PATH")]
        coverage: Option<PathBuf>,

        /// Rebase the file paths of the coverage input onto this root.
        #[arg(long, value_name = "PATH")]
        coverage_root: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, clap::Subcommand)]
pub enum TelemetryCli {
    /// Show effective telemetry state, precedence, and controls.
    Status,
    /// Enable opt-in telemetry in the user-level fallow config.
    Enable,
    /// Disable telemetry in the user-level fallow config.
    Disable,
    /// Explain inspect mode or print example payloads.
    Inspect {
        /// Print documented example payloads and field purposes.
        #[arg(long)]
        example: bool,
    },
}

#[derive(clap::Subcommand)]
pub enum CiTemplateCli {
    /// Print or vendor the GitLab CI template and MR integration helpers.
    Gitlab {
        /// Write the template and its helper scripts under DIR/ci/ instead of printing the template.
        ///
        /// Passing --vendor without a DIR writes into the current directory.
        #[arg(long, value_name = "DIR", num_args = 0..=1, default_missing_value = ".")]
        vendor: Option<PathBuf>,

        /// Overwrite existing files that differ from the bundled template.
        #[arg(long)]
        force: bool,
    },
}

/// CLI mirror of [`fallow_engine::baseline::HealthBaselineMode`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum BaselineModeArg {
    /// Match a saved health baseline per file and finding category.
    #[default]
    Count,
    /// Match a saved health baseline per function identity and finding
    /// category, so a hotspot that replaces another hotspot is reported.
    Identity,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum CiProviderArg {
    Github,
    Gitlab,
}

/// CLI mirror of [`fallow_config::TypeAwareRequire`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum TypeAwareRequireArg {
    /// Keep conservative findings and report semantic gaps.
    BestEffort,
    /// Fail the quality gate when a requested semantic query is incomplete.
    Complete,
}

#[path = "cli_args_conversions.rs"]
mod conversions;
#[path = "cli_args_health.rs"]
mod health_args;
pub use health_args::*;
#[path = "cli_args_cloud.rs"]
mod cloud_args;
#[path = "cli_args_rule_pack.rs"]
mod rule_pack_args;

pub use cloud_args::*;
pub use rule_pack_args::RulePackCli;

#[path = "cli_args_audit.rs"]
mod audit_args;
pub use audit_args::*;
#[path = "cli_args_ci.rs"]
mod ci_args;
pub use ci_args::*;
#[path = "cli_args_coverage.rs"]
mod coverage_args;
pub use coverage_args::CoverageCli;
#[path = "cli_args_report.rs"]
mod report_args;
pub use report_args::*;

#[path = "cli_args_security.rs"]
mod cli_args_security;
pub use cli_args_security::SecuritySubcommand;
#[path = "config_args.rs"]
mod config_args;
pub use config_args::{ConfigArgs, ForCommand};
#[path = "cli_args_type_aware.rs"]
mod type_aware_args;
pub use type_aware_args::TypeAwareCli;

#[cfg(test)]
#[path = "cli_args_report_tests.rs"]
mod report_flag_tests;
