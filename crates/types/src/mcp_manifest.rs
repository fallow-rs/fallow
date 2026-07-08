//! Machine-readable manifest of the tools exposed by the fallow MCP server.
//!
//! Single source of truth shared by `fallow schema` (the agent-facing
//! capability manifest in `crates/cli`) and the telemetry tool-name
//! allowlist. The MCP server itself defines tool behavior via rmcp
//! `#[tool]` attributes in `crates/mcp`; a drift test there (dev-dependency
//! on this crate) asserts this manifest stays in sync with the live tool
//! router, so the two cannot diverge silently.
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
        key_params: &["target", "production"],
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
}
