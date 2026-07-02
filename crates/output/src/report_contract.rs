use std::collections::BTreeMap;

use fallow_types::envelope::{Meta, MetaMetric, MetaRule};
use serde_json::{Value, json};

use crate::{ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION, ACTIONS_FIELD_DEFINITION};

/// Docs URL for the duplication command.
pub const DUPES_DOCS: &str = "https://docs.fallow.tools/cli/dupes";

/// Docs URL for the runtime coverage setup command's agent-readable JSON.
pub const COVERAGE_SETUP_DOCS: &str = "https://docs.fallow.tools/cli/coverage#agent-readable-json";

/// Docs URL for `fallow coverage analyze --format json --explain`.
pub const COVERAGE_ANALYZE_DOCS: &str = "https://docs.fallow.tools/cli/coverage#analyze";

/// Docs URL for the health command.
pub const HEALTH_DOCS: &str = "https://docs.fallow.tools/cli/health";

/// Docs URL for the security command.
pub const SECURITY_DOCS: &str = "https://docs.fallow.tools/cli/security";

/// Output-facing metadata for one security rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityRuleMeta<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub docs_path: &'a str,
}

/// Build the `_meta` object for `fallow health --format json --explain`.
#[must_use]
pub fn health_meta() -> Meta {
    Meta {
        docs: Some(HEALTH_DOCS.to_string()),
        field_definitions: action_field_definitions(),
        metrics: health_metrics(),
        ..Meta::default()
    }
}

/// Build the `_meta` object for `fallow security --format json --explain`.
#[must_use]
pub fn security_meta<'a>(rules: impl IntoIterator<Item = SecurityRuleMeta<'a>>) -> Meta {
    Meta {
        docs: Some(SECURITY_DOCS.to_string()),
        field_definitions: security_field_definitions(),
        metrics: BTreeMap::new(),
        rules: rules
            .into_iter()
            .map(|rule| {
                (
                    rule.id.to_string(),
                    MetaRule {
                        name: Some(rule.name.to_string()),
                        description: Some(rule.description.to_string()),
                        docs: Some(report_rule_docs_url(rule.docs_path)),
                    },
                )
            })
            .collect(),
        ..Meta::default()
    }
}

/// Build the `_meta` object for `fallow dupes --format json --explain`.
#[must_use]
pub fn dupes_meta() -> Meta {
    Meta {
        docs: Some(DUPES_DOCS.to_string()),
        field_definitions: action_field_definitions(),
        metrics: BTreeMap::from([
            (
                "duplication_percentage".to_string(),
                metric(
                    "Duplication Percentage",
                    "Fraction of total source tokens that appear in at least one clone group. Computed over the full analyzed file set.",
                    Some("[0, 100]"),
                    "lower is better",
                ),
            ),
            (
                "token_count".to_string(),
                metric(
                    "Token Count",
                    "Number of normalized source tokens in the clone group. Tokens are language-aware (keywords, identifiers, operators, punctuation). Higher token count = larger duplicate.",
                    Some("[1, ∞)"),
                    "larger clones have higher refactoring value",
                ),
            ),
            (
                "line_count".to_string(),
                metric(
                    "Line Count",
                    "Number of source lines spanned by the clone instance. Approximation of clone size for human readability.",
                    Some("[1, ∞)"),
                    "larger clones are more impactful to deduplicate",
                ),
            ),
            (
                "clone_groups".to_string(),
                metric(
                    "Clone Groups",
                    "A set of code fragments with identical or near-identical normalized token sequences. Each group has 2+ instances across different locations.",
                    None,
                    "each group is a single refactoring opportunity",
                ),
            ),
            (
                "clone_groups_below_min_occurrences".to_string(),
                metric(
                    "Clone Groups Below minOccurrences",
                    "Number of clone groups detected but hidden by the `duplicates.minOccurrences` filter. Always 0 (or absent) when the filter is at its default of 2. Pre-filter group count = `clone_groups + clone_groups_below_min_occurrences`.",
                    Some("[0, ∞)"),
                    "high values suggest noisy pair-only duplication; lower `minOccurrences` to inspect",
                ),
            ),
            (
                "clone_families".to_string(),
                metric(
                    "Clone Families",
                    "Groups of clone groups that share the same set of files. Indicates systematic duplication patterns (e.g., mirrored directory structures).",
                    None,
                    "families suggest extract-module refactoring opportunities",
                ),
            ),
        ]),
        ..Meta::default()
    }
}

/// Build the `_meta` object for `fallow coverage setup --json --explain`.
#[must_use]
pub fn coverage_setup_meta() -> Value {
    json!({
        "docs_url": COVERAGE_SETUP_DOCS,
        "field_definitions": {
            "schema_version": "Coverage setup JSON contract version. Stays at \"1\" for additive opt-in fields such as _meta.",
            "framework_detected": "Primary detected runtime framework for compatibility with single-app consumers. In workspaces this mirrors the first emitted runtime member; unknown means no runtime member was detected.",
            "package_manager": "Detected package manager used for install and run commands, or null when no package manager signal was found.",
            "runtime_targets": "Union of runtime targets across emitted members.",
            "members[]": "Per-runtime-workspace setup recipes. Pure aggregator roots and build-only libraries are omitted.",
            "members[].name": "Workspace package name from package.json, or the root directory name when package.json has no name.",
            "members[].path": "Workspace path relative to the command root. The root package is represented as \".\".",
            "members[].framework_detected": "Runtime framework detected for that member.",
            "members[].package_manager": "Package manager detected for that member, or inherited from the workspace root when no member-specific signal exists.",
            "members[].runtime_targets": "Runtime targets produced by that member.",
            "members[].files_to_edit": "Files in that member that should receive runtime beacon setup code.",
            "members[].snippets": "Copy-paste setup snippets for that member, with paths relative to the command root.",
            "members[].dockerfile_snippet": "Environment snippet for file-system capture in that member's containerized Node runtime, or null when not applicable.",
            "members[].warnings": "Actionable setup caveats discovered for that member.",
            "config_written": "Always null for --json because JSON setup is side-effect-free and never writes configuration.",
            "files_to_edit": "Compatibility copy of the primary member's files, with workspace prefixes when the primary member is not the root.",
            "snippets": "Compatibility copy of the primary member's snippets, with workspace prefixes when the primary member is not the root.",
            "dockerfile_snippet": "Environment snippet for file-system capture in containerized Node runtimes, or null when not applicable.",
            "commands": "Package-manager commands needed to install the runtime beacon and sidecar packages.",
            "next_steps": "Ordered setup workflow after applying the emitted snippets.",
            "warnings": "Actionable setup caveats discovered while building the recipe."
        },
        "enums": {
            "framework_detected": ["nextjs", "nestjs", "nuxt", "sveltekit", "astro", "remix", "vite", "plain_node", "unknown"],
            "runtime_targets": ["node", "browser"],
            "package_manager": ["npm", "pnpm", "yarn", "bun", null]
        },
        "warnings": {
            "No runtime workspace members were detected": "The root appears to be a workspace, but no runtime-bearing package was found. The payload emits install commands only.",
            "No local coverage artifact was detected yet": "Run the application with runtime coverage collection enabled, then re-run setup or health with the produced capture path.",
            "Package manager was not detected": "No packageManager field or known lockfile was found. Commands fall back to npm.",
            "Framework was not detected": "No known framework dependency or runtime script was found. Treat the recipe as a generic Node setup and adjust the entry path as needed."
        }
    })
}

/// Build the `_meta` object for `fallow coverage analyze --format json --explain`.
#[must_use]
pub fn coverage_analyze_meta() -> Value {
    json!({
        "docs_url": COVERAGE_ANALYZE_DOCS,
        "field_definitions": {
            "schema_version": "Standalone coverage analyze envelope version. \"1\" for the current shape.",
            "version": "fallow CLI version that produced this output.",
            "elapsed_ms": "Wall-clock milliseconds spent producing the report.",
            "runtime_coverage": "Same RuntimeCoverageReport block emitted by `fallow health --runtime-coverage`.",
            "runtime_coverage.summary.data_source": "Which evidence source produced the report. local = on-disk artifact via --runtime-coverage <path>; cloud = explicit pull via --cloud / --runtime-coverage-cloud / FALLOW_RUNTIME_COVERAGE_SOURCE=cloud.",
            "runtime_coverage.summary.last_received_at": "ISO-8601 timestamp of the newest runtime payload included in the report. Null for local artifacts that do not carry receipt metadata.",
            "runtime_coverage.summary.capture_quality": "Capture-window telemetry derived from the runtime evidence. lazy_parse_warning trips when more than 30% of tracked functions are V8-untracked, which usually indicates a short observation window.",
            "runtime_coverage.findings[].id": "Per-finding SUPPRESSION key (fallow:prod:<hash>). Hashes file + function + the current line, so it changes when the function moves. Use it to suppress one finding at its current location.",
            "runtime_coverage.findings[].stable_id": "Cross-surface JOIN key (fallow:fn:<hash>) from fallow_cov_protocol::function_identity_id, hashing file + name + start_line. The same function shares ONE value across findings, hot paths, blast-radius, and importance entries (the per-finding id uses a per-surface salt and differs), and across V8/Istanbul/oxc producers (columns are excluded from the hash). Like id, it changes when the function's file, name, or start line changes: it is a cross-surface/cross-producer join key, NOT a line-move-immune one. Omitted from the JSON entirely (not emitted as null) when the producing surface or an un-migrated cloud supplied no FunctionIdentity. New baselines key on this when present to align with the cross-surface join key; the grace-window reader accepts the legacy id too.",
            "runtime_coverage._matching": "Function-identity fallback order when joining runtime evidence to local static analysis: (1) exact stable_id match (fallow:fn:<hash>) when both sides carry one; (2) exact (path, name, start_line); (3) fuzzy nearest candidate within a line tolerance. Baseline suppression accepts BOTH the stable_id and the legacy fallow:prod: id during the grace window, so baselines written before this version keep suppressing.",
            "runtime_coverage.findings[].evidence.static_status": "used = the function is reachable in the AST module graph; unused = it is dead by static analysis.",
            "runtime_coverage.findings[].evidence.test_coverage": "covered = the local test suite hits the function; not_covered otherwise.",
            "runtime_coverage.findings[].evidence.v8_tracking": "tracked = V8 observed the function during the capture window; untracked otherwise.",
            "runtime_coverage.findings[].actions[].type": "Suggested follow-up identifier. delete-cold-code is emitted on safe_to_delete; review-runtime on review_required.",
            "runtime_coverage.blast_radius[]": "First-class blast-radius entries with stable fallow:blast IDs, static caller count, traffic-weighted caller reach, optional cloud deploy touch count, and low/medium/high risk band.",
            "runtime_coverage.importance[]": "First-class production-importance entries with stable fallow:importance IDs, invocations, cyclomatic complexity, owner count, 0-100 importance score, and templated reason.",
            "runtime_coverage.warnings[].code": "Stable warning identifier. cloud_functions_unmatched flags entries dropped because no AST/static counterpart was found locally."
        },
        "enums": {
            "data_source": ["local", "cloud"],
            "report_verdict": ["clean", "hot-path-touched", "cold-code-detected", "license-expired-grace", "unknown"],
            "finding_verdict": ["safe_to_delete", "review_required", "coverage_unavailable", "low_traffic", "active", "unknown"],
            "static_status": ["used", "unused"],
            "test_coverage": ["covered", "not_covered"],
            "v8_tracking": ["tracked", "untracked"],
            "action_type": ["delete-cold-code", "review-runtime"]
        },
        "warnings": {
            "no_runtime_data": "Cloud returned an empty runtime window. Either the period is too narrow or no traces have been ingested yet.",
            "cloud_functions_unmatched": "One or more cloud-side functions could not be matched against the local AST/static index and were dropped from findings. Common causes: stale runtime data after a rename/move, file path mismatch between deploy and repo, or analysis run on the wrong commit."
        }
    })
}

fn action_field_definitions() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "actions[]".to_string(),
            ACTIONS_FIELD_DEFINITION.to_string(),
        ),
        (
            "actions[].auto_fixable".to_string(),
            ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION.to_string(),
        ),
    ])
}

fn security_field_definitions() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "version".to_string(),
            "fallow CLI version that produced this output.".to_string(),
        ),
        (
            "elapsed_ms".to_string(),
            "Wall-clock milliseconds spent producing the security report.".to_string(),
        ),
        (
            "config".to_string(),
            "Privacy-safe config context relevant to security candidate generation.".to_string(),
        ),
        (
            "config.rules.*.configured".to_string(),
            "Severity from resolved config before the security command forced default-off rules on."
                .to_string(),
        ),
        (
            "config.rules.*.effective".to_string(),
            "Severity used for this security command run.".to_string(),
        ),
        (
            "config.categories_include".to_string(),
            "Configured security category include list. null means unset, [] means explicitly empty."
                .to_string(),
        ),
        (
            "config.categories_exclude".to_string(),
            "Configured security category exclude list. null means unset, [] means explicitly empty."
                .to_string(),
        ),
        (
            "security_findings[]".to_string(),
            "Unverified security candidates for downstream human or agent verification.".to_string(),
        ),
        (
            "summary.security_findings".to_string(),
            "Number of security candidates after all filters, gates, and scopes.".to_string(),
        ),
        (
            "summary.by_severity".to_string(),
            "Fixed high, medium, and low severity counts for summary JSON.".to_string(),
        ),
        (
            "summary.by_category".to_string(),
            "Candidate counts by catalogue category, or by kind for uncategorized findings."
                .to_string(),
        ),
        (
            "summary.by_reachability".to_string(),
            "Fixed reachability and source-backed ranking-signal counts for summary JSON."
                .to_string(),
        ),
        (
            "summary.by_runtime_state".to_string(),
            "Fixed production-runtime coverage state counts for summary JSON.".to_string(),
        ),
        (
            "unresolved_edge_files".to_string(),
            "Number of client files whose import cone contains dynamic edges the graph could not follow."
                .to_string(),
        ),
        (
            "unresolved_callee_sites".to_string(),
            "Number of sink-shaped nodes whose callee could not be flattened to a static path."
                .to_string(),
        ),
    ])
}

fn health_metrics() -> BTreeMap<String, MetaMetric> {
    let mut metrics = BTreeMap::new();
    metrics.extend(health_complexity_metrics());
    metrics.extend(health_churn_and_target_metrics());
    metrics.extend(health_ownership_metrics());
    metrics.extend(health_runtime_metrics());
    metrics.extend(health_styling_metrics());
    metrics
}

fn health_complexity_metrics() -> [(String, MetaMetric); 11] {
    [
        health_metric(
            "cyclomatic",
            "Cyclomatic Complexity",
            "McCabe cyclomatic complexity: 1 + number of decision points.",
            Some("[1, infinity)"),
            "lower is better; default threshold: 20",
        ),
        health_metric(
            "cognitive",
            "Cognitive Complexity",
            "Cognitive complexity penalizes nesting depth and non-linear control flow.",
            Some("[0, infinity)"),
            "lower is better; default threshold: 15",
        ),
        health_metric(
            "line_count",
            "Function Line Count",
            "Number of lines in the function body.",
            Some("[1, infinity)"),
            "context-dependent; long functions may need splitting",
        ),
        health_metric(
            "lines",
            "File Line Count",
            "Total lines of code in the file.",
            Some("[1, infinity)"),
            "context-dependent; large files may benefit from splitting",
        ),
        health_metric(
            "maintainability_index",
            "Maintainability Index",
            "Composite file score combining complexity density, dead code ratio, and coupling.",
            Some("[0, 100]"),
            "higher is better",
        ),
        health_metric(
            "complexity_density",
            "Complexity Density",
            "Total cyclomatic complexity divided by lines of code.",
            Some("[0, infinity)"),
            "lower is better; >1.0 indicates very dense complexity",
        ),
        health_metric(
            "dead_code_ratio",
            "Dead Code Ratio",
            "Fraction of value exports with zero references across the project.",
            Some("[0, 1]"),
            "lower is better; 0 means all exports are used",
        ),
        health_metric(
            "fan_in",
            "Fan-in (Importers)",
            "Number of files that import this file.",
            Some("[0, infinity)"),
            "context-dependent; high fan-in files need careful review",
        ),
        health_metric(
            "fan_out",
            "Fan-out (Imports)",
            "Number of files this file directly imports.",
            Some("[0, infinity)"),
            "lower is better; high fan-out indicates coupling",
        ),
        health_metric(
            "max_render_fan_in",
            "Render Fan-in (Blast Radius)",
            "Highest distinct-parent render count across React or Preact components.",
            Some("[0, infinity)"),
            "descriptive only; high values mean broad edit ripple",
        ),
        health_metric(
            "crap_max",
            "Untested Complexity Risk (CRAP)",
            "Highest Change Risk Anti-Patterns score from complexity and coverage evidence.",
            Some("[1, infinity)"),
            "lower is better; high values indicate complex untested code",
        ),
    ]
}

fn health_churn_and_target_metrics() -> [(String, MetaMetric); 8] {
    [
        health_metric(
            "score",
            "Hotspot Score",
            "Normalized churn multiplied by normalized complexity.",
            Some("[0, 100]"),
            "higher means riskier; prioritize refactoring high-score files",
        ),
        health_metric(
            "weighted_commits",
            "Weighted Commits",
            "Recency-weighted commit count using exponential decay.",
            Some("[0, infinity)"),
            "higher means more recent churn activity",
        ),
        health_metric(
            "trend",
            "Churn Trend",
            "Compares recent vs older commit frequency within the analysis window.",
            None,
            "accelerating files need attention; cooling files are stabilizing",
        ),
        health_metric(
            "priority",
            "Refactoring Priority",
            "Weighted refactoring score using complexity, hotspots, dead code, fan-in, and fan-out.",
            Some("[0, 100]"),
            "higher means more urgent to refactor",
        ),
        health_metric(
            "efficiency",
            "Efficiency Score",
            "Priority divided by effort estimate.",
            Some("[0, 100]"),
            "higher means better quick-win value",
        ),
        health_metric(
            "effort",
            "Effort Estimate",
            "Heuristic effort estimate based on file size, function count, and fan-in.",
            None,
            "low means quick win, high needs planning and coordination",
        ),
        health_metric(
            "confidence",
            "Confidence Level",
            "Reliability of the recommendation based on data source.",
            None,
            "high means act on it; medium or low means verify context",
        ),
        health_metric(
            "health_score",
            "Health Score",
            "Project-level aggregate score computed from vital signs and issue signals.",
            Some("[0, 100]"),
            "higher is better; missing metrics are not penalized",
        ),
    ]
}

fn health_ownership_metrics() -> [(String, MetaMetric); 6] {
    [
        health_metric(
            "bus_factor",
            "Bus Factor",
            "Minimum number of contributors who account for most recent weighted commits.",
            Some("[1, infinity)"),
            "lower is higher knowledge-loss risk",
        ),
        health_metric(
            "contributor_count",
            "Contributor Count",
            "Number of distinct authors who touched this file in the analysis window.",
            Some("[0, infinity)"),
            "higher generally indicates broader knowledge spread",
        ),
        health_metric(
            "share",
            "Contributor Share",
            "Recency-weighted share of total weighted commits attributed to a contributor.",
            Some("[0, 1]"),
            "share close to 1.0 indicates ownership concentration",
        ),
        health_metric(
            "stale_days",
            "Stale Days",
            "Days since this contributor last touched the file.",
            Some("[0, infinity)"),
            "high stale days can indicate ownership drift",
        ),
        health_metric(
            "drift",
            "Ownership Drift",
            "Whether original authorship and current contribution ownership have diverged.",
            None,
            "true means current review ownership may differ from original ownership",
        ),
        health_metric(
            "unowned",
            "Unowned (Tristate)",
            "Whether CODEOWNERS exists but has no matching owner for this file.",
            None,
            "true on a hotspot is a review-bottleneck risk",
        ),
    ]
}

fn health_runtime_metrics() -> [(String, MetaMetric); 5] {
    [
        health_metric(
            "runtime_coverage_verdict",
            "Runtime Coverage Verdict",
            "Overall verdict across runtime-coverage findings.",
            None,
            "cold-code-detected is the primary standalone cleanup signal",
        ),
        health_metric(
            "runtime_coverage_state",
            "Runtime Coverage State",
            "Per-function runtime observation state.",
            None,
            "never-called with static unused is the highest-confidence delete signal",
        ),
        health_metric(
            "runtime_coverage_confidence",
            "Runtime Coverage Confidence",
            "Confidence in a runtime-coverage finding.",
            None,
            "high means act on it; medium or low means verify context",
        ),
        health_metric(
            "production_invocations",
            "Production Invocations",
            "Observed invocation count for the function over the collected coverage window.",
            Some("[0, infinity)"),
            "0 plus tracked means cold path; high means active path",
        ),
        health_metric(
            "percent_dead_in_production",
            "Percent Dead in Production",
            "Fraction of tracked functions with zero observed invocations, multiplied by 100.",
            Some("[0, 100]"),
            "lower is better",
        ),
    ]
}

fn health_styling_metrics() -> [(String, MetaMetric); 10] {
    [
        health_metric(
            "styling_health.score",
            "Styling Health Score",
            "CSS/styling-axis aggregate score computed from the styling penalty rubric. Present only under --css.",
            Some("[0, 100]"),
            "higher is better; missing metrics are not penalized",
        ),
        health_metric(
            "styling_health.formula_version",
            "Styling Health Formula Version",
            "Version of the styling-health scoring rubric used to produce the score. Present only under --css.",
            Some("[1, infinity)"),
            "bump signals a rubric change; compare scores only within the same version",
        ),
        health_metric(
            "styling_health.penalties.duplication",
            "Styling Duplication Penalty",
            "Points deducted for copy-paste declaration blocks, scaled by the share of declarations removable via consolidation. Present only under --css.",
            Some("[0, 20]"),
            "lower is better; 0 means no removable duplicate blocks",
        ),
        health_metric(
            "styling_health.penalties.dead_surface",
            "Styling Dead-Surface Penalty",
            "Points deducted for unreferenced classes, unused tokens, at-rules, and font-faces, normalized per stylesheet. Present only under --css.",
            Some("[0, 20]"),
            "lower is better; 0 means no dead styling surface",
        ),
        health_metric(
            "styling_health.penalties.broken_references",
            "Styling Broken-References Penalty",
            "Points deducted for markup classes one edit from a defined class and animations referencing undefined keyframes. Present only under --css.",
            Some("[0, 15]"),
            "lower is better; 0 means no broken references",
        ),
        health_metric(
            "styling_health.penalties.token_erosion",
            "Styling Token-Erosion Penalty",
            "Points deducted for mixing font-size units past a healthy baseline and Tailwind arbitrary-value bypasses. Present only under --css.",
            Some("[0, 10]"),
            "lower is better; 0 means a single source of truth for the scale",
        ),
        health_metric(
            "styling_health.penalties.structural",
            "Styling Structural Penalty",
            "Points deducted for !important density above a healthy floor and deep style-rule nesting. Present only under --css.",
            Some("[0, 10]"),
            "lower is better; 0 means no structural smells",
        ),
        health_metric(
            "css_analytics.summary.near_duplicate_theme_tokens",
            "Near-Duplicate Theme Tokens",
            "Count of Tailwind v4 theme tokens whose comparable values are close to another token in the same theme dictionary. Present only in deep CSS analysis.",
            Some("[0, infinity)"),
            "0 means no near-duplicate token candidates were found",
        ),
        health_metric(
            "styling_findings[].blast_radius",
            "Styling Finding Blast Radius",
            "Static lower-bound count of known consumers affected by a styling finding. Omitted when the family has no reliable blast-radius model.",
            Some("[0, infinity)"),
            "0 means no static consumers were found; omitted means unknown",
        ),
        health_metric(
            "styling_findings[].nearest_token.distance",
            "Nearest Styling Token Distance",
            "Distance between a token-drift finding and its nearest comparable token. Units depend on the token namespace.",
            Some("(0, infinity)"),
            "lower means closer; compare only within the same token namespace",
        ),
    ]
}

fn health_metric(
    key: impl Into<String>,
    name: impl Into<String>,
    description: impl Into<String>,
    range: Option<&str>,
    interpretation: impl Into<String>,
) -> (String, MetaMetric) {
    (key.into(), metric(name, description, range, interpretation))
}

fn metric(
    name: impl Into<String>,
    description: impl Into<String>,
    range: Option<&str>,
    interpretation: impl Into<String>,
) -> MetaMetric {
    MetaMetric {
        name: Some(name.into()),
        description: Some(description.into()),
        range: range.map(str::to_string),
        interpretation: Some(interpretation.into()),
    }
}

fn report_rule_docs_url(docs_path: &str) -> String {
    format!("https://docs.fallow.tools/{docs_path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dupes_meta_uses_output_contract_shape() {
        let meta = dupes_meta();
        assert_eq!(meta.docs.as_deref(), Some(DUPES_DOCS));
        assert!(meta.field_definitions.contains_key("actions[]"));
        assert!(meta.metrics.contains_key("duplication_percentage"));
        assert!(
            meta.metrics
                .contains_key("clone_groups_below_min_occurrences")
        );
    }

    #[test]
    fn health_meta_uses_output_contract_shape() {
        let meta = health_meta();
        assert_eq!(meta.docs.as_deref(), Some(HEALTH_DOCS));
        assert!(meta.field_definitions.contains_key("actions[]"));
        assert!(meta.metrics.contains_key("cyclomatic"));
        assert!(meta.metrics.contains_key("health_score"));
        assert!(meta.metrics.contains_key("max_render_fan_in"));
        assert!(meta.metrics.contains_key("percent_dead_in_production"));
        assert!(meta.metrics.contains_key("styling_health.score"));
        assert!(
            meta.metrics
                .contains_key("styling_health.penalties.duplication")
        );
        assert!(
            meta.metrics
                .contains_key("styling_health.penalties.structural")
        );
    }

    #[test]
    fn security_meta_uses_output_contract_shape() {
        let meta = security_meta([SecurityRuleMeta {
            id: "security/example",
            name: "Example",
            description: "Example security candidate.",
            docs_path: "cli/security",
        }]);
        assert_eq!(meta.docs.as_deref(), Some(SECURITY_DOCS));
        assert!(meta.field_definitions.contains_key("security_findings[]"));
        assert!(meta.metrics.is_empty());
        assert_eq!(
            meta.rules["security/example"].docs.as_deref(),
            Some("https://docs.fallow.tools/cli/security")
        );
    }

    #[test]
    fn coverage_setup_meta_uses_output_contract_shape() {
        let meta = coverage_setup_meta();
        assert_eq!(meta["docs_url"], COVERAGE_SETUP_DOCS);
        assert!(meta["field_definitions"]["members[]"].is_string());
        assert!(meta["enums"]["runtime_targets"].is_array());
        assert!(meta["warnings"]["Package manager was not detected"].is_string());
    }

    #[test]
    fn coverage_analyze_meta_uses_output_contract_shape() {
        let meta = coverage_analyze_meta();
        assert_eq!(meta["docs_url"], COVERAGE_ANALYZE_DOCS);
        assert!(meta["field_definitions"]["runtime_coverage.findings[].stable_id"].is_string());
        assert!(meta["enums"]["action_type"].is_array());
        assert!(meta["warnings"]["cloud_functions_unmatched"].is_string());
    }
}
