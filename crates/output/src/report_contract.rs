use std::collections::BTreeMap;

use fallow_types::envelope::{Meta, MetaMetric, MetaRule};

use crate::{ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION, ACTIONS_FIELD_DEFINITION};

/// Docs URL for the duplication command.
pub const DUPES_DOCS: &str = "https://docs.fallow.tools/cli/dupes";

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
}
