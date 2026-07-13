//! Machine-readable manifest of the tools exposed by the fallow MCP server.
//!
//! Single source of truth shared by `fallow schema` (the agent-facing
//! capability manifest in `crates/cli`) and the telemetry tool-name
//! allowlist. The MCP server itself defines full wire descriptions in method
//! docs consumed by rmcp `#[tool]` attributes in `crates/mcp`; drift tests
//! there assert this manifest stays in sync with the live tool router while
//! keeping the two description roles distinct.
//!
//! The one-line `description` strings here are intentional, agent-facing
//! prose authored for the capability manifest. They deliberately do NOT
//! duplicate the longer rmcp tool descriptions in `crates/mcp` (those are
//! the MCP wire surface; these are the introspection surface). The
//! `cli_command` field carries the nearest CLI analogue so agents have one
//! manifest-backed fallback when MCP is unavailable.

/// License tier required to use a tool's full functionality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpToolLicense {
    /// Fully free; no license involved.
    Free,
    /// A free tier exists (single local capture); continuous or
    /// multi-capture use requires an active license.
    Freemium,
}

impl McpToolLicense {
    /// Kebab-case wire value for JSON output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Freemium => "freemium",
        }
    }
}

/// Static metadata for one MCP tool.
#[derive(Debug, Clone, Copy)]
pub struct McpToolInfo {
    /// Wire tool name (matches the rmcp `#[tool]` method name).
    pub name: &'static str,
    /// Coarse grouping for doc generators: `analysis`, `trace`, `fix`,
    /// `introspection`, `runtime-coverage`, or `composition`.
    pub kind: &'static str,
    /// One-line agent-facing description (fresh prose, not the rmcp
    /// description string).
    pub description: &'static str,
    /// Nearest CLI command for agents that need to fall back from MCP to a
    /// subprocess. `None` is reserved for composition-only tools with no CLI
    /// analogue.
    pub cli_command: Option<&'static str>,
    /// Distinctive parameters (a deliberate subset; the live MCP input
    /// schema is authoritative for the full parameter list).
    pub key_params: &'static [&'static str],
    /// License tier.
    pub license: McpToolLicense,
    /// Free/paid nuance; populated exactly when `license` is `Freemium`.
    pub license_note: Option<&'static str>,
    /// Whether the tool leaves the project untouched (only `fix_apply`
    /// mutates files).
    pub read_only: bool,
}

/// Free/paid nuance attached to runtime-coverage capabilities. Shared with
/// the `fallow schema` issue-type rows so the wording cannot drift.
pub const RUNTIME_COVERAGE_LICENSE_NOTE: &str = "A single local runtime-coverage capture is free; continuous or multi-capture runtime monitoring requires an active license (fallow license activate).";

/// All tools exposed by the fallow MCP server, in registration order.
pub const MCP_TOOLS: &[McpToolInfo] = &[
    McpToolInfo {
        name: "code_execute",
        kind: "composition",
        description: "Run a bounded read-only JavaScript snippet that composes fallow's analysis tools inside a sandbox (Code Mode meta-tool, not a plain analysis call)",
        cli_command: None,
        key_params: &["code", "timeout_ms", "max_output_bytes"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "analyze",
        kind: "analysis",
        description: "Full dead-code analysis: unused files, exports, types, dependencies, circular dependencies, and boundary violations",
        cli_command: Some("fallow dead-code --format json --quiet"),
        key_params: &[
            "issue_types",
            "production",
            "workspace",
            "baseline",
            "group_by",
            "file",
        ],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "check_changed",
        kind: "analysis",
        description: "Incremental dead-code analysis scoped to files changed since a git ref (ideal for PR review)",
        cli_command: Some("fallow dead-code --changed-since <ref> --format json --quiet"),
        key_params: &["since", "baseline", "fail_on_regression"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "security_candidates",
        kind: "analysis",
        description: "Unverified local security candidates (tainted sinks) for downstream agent verification",
        cli_command: Some("fallow security --format json --quiet"),
        key_params: &["gate", "surface", "changed_since", "paths"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "inspect_target",
        kind: "analysis",
        description: "One evidence bundle for a file or exported symbol: trace, dead-code actions, duplication, complexity, and security candidates",
        cli_command: Some("fallow inspect --format json --quiet"),
        key_params: &["target", "production", "include_churn"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "guard",
        kind: "introspection",
        description: "Report the architecture rules that apply to given files before editing them: boundary zone, allowed import zones, forbidden calls, and rule-pack policies",
        cli_command: Some("fallow guard <file> --format json --quiet"),
        key_params: &["files"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "find_dupes",
        kind: "analysis",
        description: "Code duplication detection with clone groups and refactoring suggestions",
        cli_command: Some("fallow dupes --format json --quiet"),
        key_params: &["mode", "min_tokens", "min_occurrences", "top", "threshold"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "check_health",
        kind: "analysis",
        description: "Complexity, styling health, hotspots, ownership, refactoring targets, coverage gaps, and CSS/CSS-in-JS candidates",
        cli_command: Some("fallow health --format json --quiet"),
        key_params: &[
            "score",
            "css",
            "file_scores",
            "hotspots",
            "targets",
            "coverage",
            "runtime_coverage",
            "max_crap",
            "group_by",
        ],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "check_runtime_coverage",
        kind: "runtime-coverage",
        description: "Merge V8 or Istanbul runtime coverage into the health report (hot paths, cold paths, verdicts)",
        cli_command: Some("fallow health --runtime-coverage <path> --format json --quiet"),
        key_params: &[
            "coverage",
            "min_invocations_hot",
            "min_observation_volume",
            "low_traffic_threshold",
            "group_by",
        ],
        license: McpToolLicense::Freemium,
        license_note: Some(RUNTIME_COVERAGE_LICENSE_NOTE),
        read_only: true,
    },
    McpToolInfo {
        name: "get_hot_paths",
        kind: "runtime-coverage",
        description: "Production hot paths from runtime coverage, sorted by invocation volume",
        cli_command: Some("fallow health --runtime-coverage <path> --format json --quiet"),
        key_params: &["coverage", "top", "min_invocations_hot"],
        license: McpToolLicense::Freemium,
        license_note: Some(RUNTIME_COVERAGE_LICENSE_NOTE),
        read_only: true,
    },
    McpToolInfo {
        name: "get_blast_radius",
        kind: "runtime-coverage",
        description: "Blast-radius context (caller counts, risk bands) from runtime coverage; augments review only, never gates safe_to_delete (three-state tracking issues that verdict)",
        cli_command: Some("fallow health --runtime-coverage <path> --format json --quiet"),
        key_params: &["coverage", "group_by"],
        license: McpToolLicense::Freemium,
        license_note: Some(RUNTIME_COVERAGE_LICENSE_NOTE),
        read_only: true,
    },
    McpToolInfo {
        name: "get_importance",
        kind: "runtime-coverage",
        description: "Production-importance scores (0-100) combining invocations, complexity, and ownership; augments review only, never gates safe_to_delete (three-state tracking issues that verdict)",
        cli_command: Some("fallow health --runtime-coverage <path> --format json --quiet"),
        key_params: &["coverage", "group_by"],
        license: McpToolLicense::Freemium,
        license_note: Some(RUNTIME_COVERAGE_LICENSE_NOTE),
        read_only: true,
    },
    McpToolInfo {
        name: "get_cleanup_candidates",
        kind: "runtime-coverage",
        description: "Cleanup candidates with safe_to_delete, review_required, and low_traffic verdicts from runtime coverage",
        cli_command: Some("fallow health --runtime-coverage <path> --format json --quiet"),
        key_params: &["coverage", "group_by"],
        license: McpToolLicense::Freemium,
        license_note: Some(RUNTIME_COVERAGE_LICENSE_NOTE),
        read_only: true,
    },
    McpToolInfo {
        name: "get_token_blast_radius",
        kind: "analysis",
        description: "Design-token blast radius for Tailwind v4 @theme tokens AND CSS-in-JS defineVars/createTheme-family token definitions: per token, a consumer_count (static lower bound) and a capped located consumers[] sample tagged theme-var/css-var/utility/apply (Tailwind) or js-member (CSS-in-JS cross-module member access); descriptive context for sizing a token change, never a deletion gate",
        cli_command: Some("fallow health --css --format json --quiet"),
        key_params: &[],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "audit",
        kind: "analysis",
        description: "Combined dead-code, complexity, duplication, and styling audit for changed files with a pass/warn/fail verdict",
        cli_command: Some("fallow audit --format json --quiet"),
        key_params: &[
            "gate",
            "base",
            "css_deep",
            "max_crap",
            "coverage",
            "runtime_coverage",
        ],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "decision_surface",
        kind: "analysis",
        description: "Surface the few consequential structural decisions a change embeds (coupling, public API, dependency), each as a judgment question with the routed expert; ranked, capped, and signal_id-anchored",
        cli_command: Some("fallow decision-surface --format json --quiet"),
        key_params: &["base", "max_decisions", "workspace"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "fallow_explain",
        kind: "introspection",
        description: "Explain one issue type (rationale, examples, fix guidance) without running an analysis",
        cli_command: Some("fallow explain <issue-type> --format json --quiet"),
        key_params: &["issue_type"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "fix_preview",
        kind: "fix",
        description: "Dry-run auto-fix preview; shows what would change without modifying files",
        cli_command: Some("fallow fix --dry-run --format json --quiet"),
        key_params: &["no_create_config"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "fix_apply",
        kind: "fix",
        description: "Apply auto-fixes: removes unused exports, dependencies, and enum members (mutates files)",
        cli_command: Some("fallow fix --yes --format json --quiet"),
        key_params: &["no_create_config"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: false,
    },
    McpToolInfo {
        name: "project_info",
        kind: "introspection",
        description: "Project metadata: active framework plugins, discovered files, entry points, and boundary zones",
        cli_command: Some("fallow list --files --entry-points --plugins --format json --quiet"),
        key_params: &["entry_points", "files", "plugins", "boundaries"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "recommend",
        kind: "introspection",
        description: "Recommend a project-tailored config from framework/workspace/tooling detection: a loader-validated proposed_config and three-valued auto/default/taste decisions for cold-start onboarding",
        cli_command: Some("fallow recommend --format json --quiet"),
        key_params: &["root"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "list_boundaries",
        kind: "introspection",
        description: "List architecture boundary zones and access rules",
        cli_command: Some("fallow list --boundaries --format json --quiet"),
        key_params: &[],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "feature_flags",
        kind: "analysis",
        description: "Detect feature flag patterns (environment variables, SDK calls, config objects)",
        cli_command: Some("fallow flags --format json --quiet"),
        // flag_type / confidence exist on the schema but are not yet
        // forwarded by the arg builder (CLI filter pending); list only
        // params that actually take effect.
        key_params: &["workspace", "production"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "list_suppressions",
        kind: "analysis",
        description: "List active fallow-ignore suppression markers grouped per file (line, kind, level, reason, and a stale cross-reference); a read-only governance inventory that always exits 0",
        cli_command: Some("fallow suppressions --format json --quiet"),
        key_params: &["workspace", "changed_since", "file"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "impact",
        kind: "introspection",
        description: "Read the local Fallow Impact value-tracking report (per-project history in the user config dir, never in the repo; local-dev only)",
        cli_command: Some("fallow impact --format json --quiet"),
        key_params: &["root"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "impact_all",
        kind: "introspection",
        description: "Roll every tracked fallow project on this machine into one cross-repo value report (hashed keys plus basename labels, never paths; local-dev only)",
        cli_command: Some("fallow impact --all --format json --quiet"),
        key_params: &["sort", "limit"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "trace_export",
        kind: "trace",
        description: "Trace why an export is used or unused, including re-export chains and entry-point status",
        cli_command: Some("fallow dead-code --trace <file:export> --format json --quiet"),
        key_params: &["file", "export_name"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "trace_file",
        kind: "trace",
        description: "Trace all module-graph edges for a file (imports, exports, importers, re-exports)",
        cli_command: Some("fallow dead-code --trace-file <file> --format json --quiet"),
        key_params: &["file"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "impact_closure",
        kind: "trace",
        description: "Trace the transitive affected-but-not-in-diff set and coordination gaps for one file",
        cli_command: Some("fallow dead-code --impact-closure <path> --format json --quiet"),
        key_params: &["path"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "trace_dependency",
        kind: "trace",
        description: "Trace where a dependency is imported and whether scripts or CI use it",
        cli_command: Some("fallow dead-code --trace-dependency <package> --format json --quiet"),
        key_params: &["package_name"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
    McpToolInfo {
        name: "trace_clone",
        kind: "trace",
        description: "Deep-dive a duplicate-code clone group by location or fingerprint",
        cli_command: Some("fallow dupes --trace <file:line> --format json --quiet"),
        key_params: &["file", "line", "fingerprint"],
        license: McpToolLicense::Free,
        license_note: None,
        read_only: true,
    },
];

/// One row of the cross-surface capability parity table: how a single fallow
/// capability is (or is deliberately not) exposed on the three agent execution
/// surfaces.
///
/// The three surfaces:
/// - `api_runner`: the `fallow_api::run_*` entry point (the in-process Rust API
///   that both the napi addon and the MCP server's in-process path build on).
/// - `napi_export`: the `#[napi(js_name = ...)]` function in `crates/napi` (the
///   `@fallow/node` addon surface).
/// - `mcp_tool`: a tool `name` in [`MCP_TOOLS`] (the MCP server surface).
///
/// `omission_note` is REQUIRED exactly when at least one surface is `None`; it
/// states the intent behind the asymmetry (folded into another tool, CLI-only,
/// no fix surface on the addon, and so on). Rows where all three surfaces are
/// `Some` are the only capabilities exposed first-class on every surface, and
/// carry no note.
///
/// This table is drift-tested from all three sides: [`MCP_TOOLS`] here (every
/// tool appears in exactly one row), the `#[napi]` exports in `crates/napi`,
/// and the public `run_*` re-exports in `crates/api`. It is a hand-authored
/// statement of product intent, not a generated artifact: each note is a claim
/// the maintainer endorses.
#[derive(Debug, Clone, Copy)]
pub struct CapabilityParityRow {
    /// Human-readable capability name. Table key for readers; not a wire value.
    pub capability: &'static str,
    /// `fallow_api::run_*` entry point, when the capability is a programmatic
    /// runner.
    pub api_runner: Option<&'static str>,
    /// `#[napi(js_name = ...)]` export, when the addon exposes the capability.
    pub napi_export: Option<&'static str>,
    /// [`MCP_TOOLS`] `name`, when the MCP server exposes a dedicated tool.
    pub mcp_tool: Option<&'static str>,
    /// Intent behind a surface omission. `Some` exactly when a surface above is
    /// `None`.
    pub omission_note: Option<&'static str>,
}

/// Cross-surface capability parity: which of the three agent execution surfaces
/// (api runner / napi export / MCP tool) exposes each fallow capability, and why
/// a surface is deliberately absent.
///
/// Only three capabilities are first-class on ALL three surfaces (dead-code,
/// duplication, feature-flags: the aligned primitives). The napi addon ships a
/// deliberately narrow set of seven whole-project analysis primitives and has NO
/// fix, trace, impact, audit, or introspection surface; the MCP server is the
/// broad agent surface and folds several api/napi primitives (circular deps,
/// boundary violations, complexity, health-runner) into `analyze` / `check_health`
/// rather than shipping dedicated tools.
pub const CAPABILITY_PARITY: &[CapabilityParityRow] = &[
    // -- Aligned primitives: first-class on all three surfaces (no note). --
    CapabilityParityRow {
        capability: "dead-code analysis",
        api_runner: Some("run_dead_code"),
        napi_export: Some("detectDeadCode"),
        mcp_tool: Some("analyze"),
        omission_note: None,
    },
    CapabilityParityRow {
        capability: "code duplication",
        api_runner: Some("run_duplication"),
        napi_export: Some("detectDuplication"),
        mcp_tool: Some("find_dupes"),
        omission_note: None,
    },
    CapabilityParityRow {
        capability: "feature-flag detection",
        api_runner: Some("run_feature_flags"),
        napi_export: Some("detectFeatureFlags"),
        mcp_tool: Some("feature_flags"),
        omission_note: None,
    },
    // -- api + napi, folded into a broader MCP tool (no dedicated MCP tool). --
    CapabilityParityRow {
        capability: "circular dependencies",
        api_runner: Some("run_circular_dependencies"),
        napi_export: Some("detectCircularDependencies"),
        mcp_tool: None,
        omission_note: Some(
            "No dedicated MCP tool: circular dependencies ship inside the `analyze` dead-code result. Exposed standalone on the api and napi surfaces.",
        ),
    },
    CapabilityParityRow {
        capability: "boundary violations",
        api_runner: Some("run_boundary_violations"),
        napi_export: Some("detectBoundaryViolations"),
        mcp_tool: None,
        omission_note: Some(
            "No dedicated MCP tool: boundary violations ship inside `analyze`; the MCP `guard` tool is the file-scoped architecture gate and shells out to the CLI rather than calling this runner. Exposed standalone on the api and napi surfaces.",
        ),
    },
    CapabilityParityRow {
        capability: "complexity",
        api_runner: Some("run_complexity_with_runner"),
        napi_export: Some("computeComplexity"),
        mcp_tool: None,
        omission_note: Some(
            "No dedicated MCP tool: complexity ships inside `check_health`'s health report. Exposed standalone on the api (runner-injected) and napi (computeComplexity) surfaces.",
        ),
    },
    CapabilityParityRow {
        capability: "health (runner-injected entry point)",
        api_runner: Some("run_health_with_runner"),
        napi_export: Some("computeHealth"),
        mcp_tool: None,
        omission_note: Some(
            "Runner-injected health entry point that napi's computeHealth binds. The MCP surface for health is `check_health`, which uses the run_health convenience wrapper; same capability, different entry point.",
        ),
    },
    // -- MCP tool + api runner, no napi export (addon ships only the seven
    //    whole-project primitives). --
    CapabilityParityRow {
        capability: "health / hotspots",
        api_runner: Some("run_health"),
        napi_export: None,
        mcp_tool: Some("check_health"),
        omission_note: Some(
            "napi exposes health through computeHealth, which binds the runner-injected run_health_with_runner variant (its own row); this row is the MCP `check_health` tool plus the run_health convenience wrapper.",
        ),
    },
    CapabilityParityRow {
        capability: "changed-scope dead code",
        api_runner: Some("run_dead_code"),
        napi_export: None,
        mcp_tool: Some("check_changed"),
        omission_note: Some(
            "Changed-scope dead code: reuses run_dead_code with a git ref. No napi export; the addon ships whole-project primitives, not git-scoped variants.",
        ),
    },
    CapabilityParityRow {
        capability: "changed-code review gate",
        api_runner: Some("run_audit"),
        napi_export: None,
        mcp_tool: Some("audit"),
        omission_note: Some(
            "Changed-code review gate. No napi export; the audit pipeline (git base resolution, sub-analyses) is exposed on the CLI, MCP, and api surfaces only.",
        ),
    },
    CapabilityParityRow {
        capability: "decision surface",
        api_runner: Some("run_decision_surface"),
        napi_export: None,
        mcp_tool: Some("decision_surface"),
        omission_note: Some(
            "Changed-code decision surface. No napi export; the changed-code pipeline is CLI, MCP, and api only.",
        ),
    },
    CapabilityParityRow {
        capability: "project info",
        api_runner: Some("run_project_info"),
        napi_export: None,
        mcp_tool: Some("project_info"),
        omission_note: Some(
            "Project discovery (files, entry points, plugins). No napi export; introspection is CLI, MCP, and api only.",
        ),
    },
    CapabilityParityRow {
        capability: "boundary listing",
        api_runner: Some("run_list_boundaries"),
        napi_export: None,
        mcp_tool: Some("list_boundaries"),
        omission_note: Some(
            "Architecture boundary listing. No napi export; introspection is CLI, MCP, and api only.",
        ),
    },
    CapabilityParityRow {
        capability: "export usage trace",
        api_runner: Some("run_trace_export"),
        napi_export: None,
        mcp_tool: Some("trace_export"),
        omission_note: Some(
            "Export usage trace. No napi export; the trace family is CLI, MCP, and api only (and emits an un-enveloped programmatic shape, see the plan-028 schema-conformance note).",
        ),
    },
    CapabilityParityRow {
        capability: "file edge trace",
        api_runner: Some("run_trace_file"),
        napi_export: None,
        mcp_tool: Some("trace_file"),
        omission_note: Some(
            "File edge trace (imports, exports, importers). No napi export; the trace family is CLI, MCP, and api only.",
        ),
    },
    CapabilityParityRow {
        capability: "dependency usage trace",
        api_runner: Some("run_trace_dependency"),
        napi_export: None,
        mcp_tool: Some("trace_dependency"),
        omission_note: Some(
            "Dependency usage trace. No napi export; the trace family is CLI, MCP, and api only.",
        ),
    },
    CapabilityParityRow {
        capability: "clone trace",
        api_runner: Some("run_trace_clone"),
        napi_export: None,
        mcp_tool: Some("trace_clone"),
        omission_note: Some(
            "Duplicate-clone deep dive. No napi export; the trace family is CLI, MCP, and api only.",
        ),
    },
    // -- api-only: no napi export and no dedicated MCP tool. --
    CapabilityParityRow {
        capability: "combined report",
        api_runner: Some("run_combined"),
        napi_export: None,
        mcp_tool: None,
        omission_note: Some(
            "api-only: the bare `fallow` combined report (dead-code + dupes + health). No napi export and no MCP tool; agents compose the three primitives (analyze / find_dupes / check_health) instead.",
        ),
    },
    // -- MCP-only tools that shell out to the CLI: no api runner, no napi
    //    export. --
    CapabilityParityRow {
        capability: "Code Mode composition",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("code_execute"),
        omission_note: Some(
            "Code Mode composition meta-tool: runs a sandboxed JS snippet that composes other tools. MCP-only orchestration surface; no api runner and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "security candidates",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("security_candidates"),
        omission_note: Some(
            "Security candidate surfacing. MCP shells out to `fallow security`; no fallow_api run_* runner and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "target inspection",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("inspect_target"),
        omission_note: Some(
            "Single-target evidence bundle. MCP shells out to `fallow inspect`; served by a bespoke serializer rather than a run_* runner, and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "architecture guard",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("guard"),
        omission_note: Some(
            "File-scoped architecture gate. MCP shells out to `fallow guard`; the programmatic boundary surface is run_boundary_violations / detectBoundaryViolations (the boundary-violations row), not this tool.",
        ),
    },
    CapabilityParityRow {
        capability: "runtime-coverage merge",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("check_runtime_coverage"),
        omission_note: Some(
            "Runtime-coverage merge into health. MCP shells out to `fallow health --runtime-coverage`; no dedicated run_* runner (run_health covers base health) and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "runtime-coverage hot paths",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("get_hot_paths"),
        omission_note: Some(
            "Runtime-coverage hot-path view. MCP shells out to `fallow health --runtime-coverage`; no dedicated run_* runner and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "runtime-coverage blast radius",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("get_blast_radius"),
        omission_note: Some(
            "Runtime-coverage blast-radius view. MCP shells out to `fallow health --runtime-coverage`; no dedicated run_* runner and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "runtime-coverage importance",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("get_importance"),
        omission_note: Some(
            "Runtime-coverage importance view. MCP shells out to `fallow health --runtime-coverage`; no dedicated run_* runner and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "runtime-coverage cleanup candidates",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("get_cleanup_candidates"),
        omission_note: Some(
            "Runtime-coverage cleanup-candidate view. MCP shells out to `fallow health --runtime-coverage`; no dedicated run_* runner and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "design-token blast radius",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("get_token_blast_radius"),
        omission_note: Some(
            "Design-token blast radius. MCP shells out to `fallow health --css`; no dedicated run_* runner and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "issue-type explanation",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("fallow_explain"),
        omission_note: Some(
            "Static issue-type explanation. Served by serialize_explain_programmatic_json (a serializer, not a run_* runner) and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "auto-fix preview",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("fix_preview"),
        omission_note: Some(
            "Auto-fix dry run. MCP shells out to `fallow fix --dry-run`; no run_* runner and no napi export (the addon has no fix surface).",
        ),
    },
    CapabilityParityRow {
        capability: "auto-fix apply",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("fix_apply"),
        omission_note: Some(
            "Auto-fix apply: the only mutating tool. MCP shells out to `fallow fix --yes`; no run_* runner and no napi export (the addon has no fix surface).",
        ),
    },
    CapabilityParityRow {
        capability: "config recommendation",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("recommend"),
        omission_note: Some(
            "Project-tailored config recommendation. MCP shells out to `fallow recommend`; no run_* runner and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "suppression inventory",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("list_suppressions"),
        omission_note: Some(
            "Active fallow-ignore inventory. MCP shells out to `fallow suppressions`; no run_* runner and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "impact ledger",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("impact"),
        omission_note: Some(
            "Local impact ledger. MCP shells out to `fallow impact`; no run_* runner and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "cross-repo impact ledger",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("impact_all"),
        omission_note: Some(
            "Cross-repo impact ledger. MCP shells out to `fallow impact --all`; no run_* runner and no napi export.",
        ),
    },
    CapabilityParityRow {
        capability: "reverse-dependency closure",
        api_runner: None,
        napi_export: None,
        mcp_tool: Some("impact_closure"),
        omission_note: Some(
            "Reverse-dependency closure for a path. MCP shells out to `fallow dead-code --impact-closure`; no dedicated run_* runner and no napi export.",
        ),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_are_unique() {
        let mut names: Vec<&str> = MCP_TOOLS.iter().map(|t| t.name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "duplicate tool name in MCP_TOOLS");
    }

    #[test]
    fn freemium_set_is_exactly_the_runtime_coverage_family() {
        let freemium: Vec<&str> = MCP_TOOLS
            .iter()
            .filter(|t| t.license == McpToolLicense::Freemium)
            .map(|t| t.name)
            .collect();
        assert_eq!(
            freemium,
            [
                "check_runtime_coverage",
                "get_hot_paths",
                "get_blast_radius",
                "get_importance",
                "get_cleanup_candidates",
            ],
            "freemium marking must cover exactly the runtime-coverage family"
        );
    }

    #[test]
    fn license_note_present_exactly_when_freemium() {
        for tool in MCP_TOOLS {
            assert_eq!(
                tool.license_note.is_some(),
                tool.license == McpToolLicense::Freemium,
                "tool {} must carry a license_note iff freemium",
                tool.name
            );
        }
    }

    #[test]
    fn kinds_are_from_the_documented_set() {
        let allowed = [
            "analysis",
            "trace",
            "fix",
            "introspection",
            "runtime-coverage",
            "composition",
        ];
        for tool in MCP_TOOLS {
            assert!(
                allowed.contains(&tool.kind),
                "tool {} has undocumented kind {}",
                tool.name,
                tool.kind
            );
        }
    }

    #[test]
    fn only_fix_apply_mutates() {
        for tool in MCP_TOOLS {
            assert_eq!(
                tool.read_only,
                tool.name != "fix_apply",
                "read_only flag wrong for {}",
                tool.name
            );
        }
    }

    #[test]
    fn descriptions_are_single_line_and_non_empty() {
        for tool in MCP_TOOLS {
            assert!(
                !tool.description.is_empty(),
                "{} has empty description",
                tool.name
            );
            assert!(
                !tool.description.contains('\n'),
                "{} description must be one line",
                tool.name
            );
        }
    }

    #[test]
    fn every_tool_has_a_cli_analogue_or_explicit_none() {
        for tool in MCP_TOOLS {
            let cli_command = tool.cli_command.unwrap_or("");
            if tool.name == "code_execute" {
                assert!(
                    tool.cli_command.is_none(),
                    "code_execute is a composition meta-tool and must not pretend to have a CLI analogue"
                );
                continue;
            }

            assert!(
                cli_command.starts_with("fallow "),
                "{} must document the nearest CLI analogue for agent fallback",
                tool.name
            );
            assert!(
                !cli_command.contains('\n'),
                "{} cli_command must be one line",
                tool.name
            );
        }
    }

    #[test]
    fn capability_parity_covers_every_mcp_tool_exactly_once() {
        use std::collections::BTreeSet;

        let table_tools: Vec<&str> = CAPABILITY_PARITY
            .iter()
            .filter_map(|row| row.mcp_tool)
            .collect();

        // No MCP tool is named in more than one row.
        let unique: BTreeSet<&str> = table_tools.iter().copied().collect();
        assert_eq!(
            unique.len(),
            table_tools.len(),
            "an MCP tool appears in more than one capability-parity row"
        );

        // The parity table's MCP column equals the live MCP_TOOLS set exactly:
        // missing => a tool is absent from the table; extra => a phantom name.
        let live: BTreeSet<&str> = MCP_TOOLS.iter().map(|tool| tool.name).collect();
        assert_eq!(
            unique, live,
            "capability-parity mcp_tool column must equal the MCP_TOOLS name set exactly"
        );
    }

    #[test]
    fn capability_parity_notes_track_surface_absence() {
        for row in CAPABILITY_PARITY {
            let any_absent =
                row.api_runner.is_none() || row.napi_export.is_none() || row.mcp_tool.is_none();
            assert_eq!(
                row.omission_note.is_some(),
                any_absent,
                "row {:?}: omission_note must be present exactly when a surface is absent",
                row.capability
            );
        }
    }

    #[test]
    fn capability_parity_napi_exports_are_unique_per_row() {
        use std::collections::BTreeSet;

        let exports: Vec<&str> = CAPABILITY_PARITY
            .iter()
            .filter_map(|row| row.napi_export)
            .collect();
        let unique: BTreeSet<&str> = exports.iter().copied().collect();
        assert_eq!(
            unique.len(),
            exports.len(),
            "a napi export appears in more than one capability-parity row"
        );
    }

    #[test]
    fn only_aligned_primitives_span_all_three_surfaces() {
        let full_parity: Vec<&str> = CAPABILITY_PARITY
            .iter()
            .filter(|row| {
                row.api_runner.is_some() && row.napi_export.is_some() && row.mcp_tool.is_some()
            })
            .map(|row| row.capability)
            .collect();
        assert_eq!(
            full_parity,
            [
                "dead-code analysis",
                "code duplication",
                "feature-flag detection"
            ],
            "the set of capabilities exposed on all three surfaces changed; update the parity \
             narrative and confirm the new alignment is intended"
        );
    }
}
