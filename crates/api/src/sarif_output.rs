//! Shared SARIF output assembly for health and duplication reports.

use std::path::{Path, PathBuf};

use fallow_output::{
    CoverageIntelligenceRecommendation, CoverageIntelligenceReport, CoverageIntelligenceVerdict,
    ExceededThreshold, FindingSeverity, HealthReport, RuntimeCoverageReport,
    RuntimeCoverageVerdict, SarifDocumentInput, SarifResultInput, build_sarif_document,
    build_sarif_result, normalize_uri,
};
use fallow_types::duplicates::{CloneGroup, DuplicationReport};
use rustc_hash::FxHashMap;

type SarifRuleBuilder<'a> = dyn Fn(&str, &str, &str) -> serde_json::Value + 'a;

#[derive(Default)]
struct SourceSnippetCache {
    files: FxHashMap<PathBuf, Vec<String>>,
}

impl SourceSnippetCache {
    fn line(&mut self, path: &Path, line: u32) -> Option<String> {
        if line == 0 {
            return None;
        }
        if !self.files.contains_key(path) {
            let lines = std::fs::read_to_string(path)
                .ok()
                .map(|source| source.lines().map(str::to_owned).collect())
                .unwrap_or_default();
            self.files.insert(path.to_path_buf(), lines);
        }
        self.files
            .get(path)
            .and_then(|lines| lines.get(line.saturating_sub(1) as usize))
            .cloned()
    }
}

/// Build SARIF output from duplication analysis results.
#[must_use]
pub fn build_duplication_sarif(
    report: &DuplicationReport,
    root: &Path,
    rule_builder: &SarifRuleBuilder<'_>,
) -> serde_json::Value {
    build_duplication_sarif_with_group(report, root, rule_builder, |_| None)
}

/// Build grouped SARIF output from duplication analysis results.
#[must_use]
pub fn build_grouped_duplication_sarif(
    report: &DuplicationReport,
    root: &Path,
    rule_builder: &SarifRuleBuilder<'_>,
    group_for_clone: impl Fn(&CloneGroup) -> String,
) -> serde_json::Value {
    build_duplication_sarif_with_group(report, root, rule_builder, |group| {
        Some(group_for_clone(group))
    })
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "line and column values are bounded by source size"
)]
fn build_duplication_sarif_with_group(
    report: &DuplicationReport,
    root: &Path,
    rule_builder: &SarifRuleBuilder<'_>,
    group_for_clone: impl Fn(&CloneGroup) -> Option<String>,
) -> serde_json::Value {
    let mut sarif_results = Vec::new();
    let mut snippets = SourceSnippetCache::default();

    for (i, group) in report.clone_groups.iter().enumerate() {
        let group_value = group_for_clone(group);
        for instance in &group.instances {
            let uri = relative_uri(&instance.file, root);
            let source_snippet = snippets.line(&instance.file, instance.start_line as u32);
            let mut result = sarif_result_with_snippet(
                "fallow/code-duplication",
                "warning",
                &format!(
                    "Code clone group {} ({} lines, {} instances)",
                    i + 1,
                    group.line_count,
                    group.instances.len()
                ),
                &uri,
                Some((instance.start_line as u32, (instance.start_col + 1) as u32)),
                source_snippet.as_deref(),
            );
            if let Some(group) = &group_value {
                set_sarif_result_property(&mut result, "group", group.clone());
            }
            sarif_results.push(result);
        }
    }

    let rules = vec![rule_builder(
        "fallow/code-duplication",
        "Duplicated code block",
        "warning",
    )];
    sarif_document(&sarif_results, &rules)
}

/// Build SARIF output from a health report.
#[must_use]
pub fn build_health_sarif(
    report: &HealthReport,
    root: &Path,
    rule_builder: &SarifRuleBuilder<'_>,
) -> serde_json::Value {
    let mut sarif_results = Vec::new();
    let mut snippets = SourceSnippetCache::default();

    append_health_sarif_results(report, root, &mut sarif_results, &mut snippets);
    let health_rules = health_sarif_rules(rule_builder);
    sarif_document(&sarif_results, &health_rules)
}

/// Add a SARIF result property by resolving each result URI through a caller.
pub fn annotate_sarif_results(
    sarif: &mut serde_json::Value,
    property: &str,
    mut value_for_uri: impl FnMut(&str) -> String,
) {
    if let Some(runs) = sarif
        .get_mut("runs")
        .and_then(serde_json::Value::as_array_mut)
    {
        for run in runs {
            if let Some(results) = run
                .get_mut("results")
                .and_then(serde_json::Value::as_array_mut)
            {
                for result in results {
                    let uri = result
                        .pointer("/locations/0/physicalLocation/artifactLocation/uri")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    let value = value_for_uri(uri);
                    set_sarif_result_property(result, property, value);
                }
            }
        }
    }
}

fn set_sarif_result_property(result: &mut serde_json::Value, key: &str, value: String) {
    let Some(result) = result.as_object_mut() else {
        return;
    };
    let props = result
        .entry("properties")
        .or_insert_with(|| serde_json::json!({}));
    let Some(props) = props.as_object_mut() else {
        return;
    };
    props.insert(key.to_string(), serde_json::Value::String(value));
}

fn append_health_sarif_results(
    report: &HealthReport,
    root: &Path,
    sarif_results: &mut Vec<serde_json::Value>,
    snippets: &mut SourceSnippetCache,
) {
    append_complexity_sarif_results(sarif_results, report, root, snippets);

    if let Some(ref production) = report.runtime_coverage {
        append_runtime_coverage_sarif_results(sarif_results, production, root, snippets);
    }
    if let Some(ref intelligence) = report.coverage_intelligence {
        append_coverage_intelligence_sarif_results(sarif_results, intelligence, root, snippets);
    }

    append_refactoring_target_sarif_results(sarif_results, report, root);
    append_coverage_gap_sarif_results(sarif_results, report, root, snippets);
}

fn health_sarif_rules(rule_builder: &SarifRuleBuilder<'_>) -> Vec<serde_json::Value> {
    let mut rules = health_complexity_sarif_rules(rule_builder);
    rules.extend(health_runtime_sarif_rules(rule_builder));
    rules.extend(health_coverage_intelligence_sarif_rules(rule_builder));
    rules
}

fn health_complexity_sarif_rules(rule_builder: &SarifRuleBuilder<'_>) -> Vec<serde_json::Value> {
    vec![
        rule_builder(
            "fallow/high-cyclomatic-complexity",
            "Function has high cyclomatic complexity",
            "note",
        ),
        rule_builder(
            "fallow/high-cognitive-complexity",
            "Function has high cognitive complexity",
            "note",
        ),
        rule_builder(
            "fallow/high-complexity",
            "Function exceeds both complexity thresholds",
            "note",
        ),
        rule_builder(
            "fallow/high-crap-score",
            "Function has a high CRAP score (high complexity combined with low coverage)",
            "warning",
        ),
        rule_builder(
            "fallow/refactoring-target",
            "File identified as a high-priority refactoring candidate",
            "warning",
        ),
    ]
}

fn health_runtime_sarif_rules(rule_builder: &SarifRuleBuilder<'_>) -> Vec<serde_json::Value> {
    vec![
        rule_builder(
            "fallow/untested-file",
            "Runtime-reachable file has no test dependency path",
            "warning",
        ),
        rule_builder(
            "fallow/untested-export",
            "Runtime-reachable export has no test dependency path",
            "warning",
        ),
        rule_builder(
            "fallow/runtime-safe-to-delete",
            "Function is statically unused and was never invoked in production",
            "warning",
        ),
        rule_builder(
            "fallow/runtime-review-required",
            "Function is statically used but was never invoked in production",
            "warning",
        ),
        rule_builder(
            "fallow/runtime-low-traffic",
            "Function was invoked below the low-traffic threshold relative to total trace count",
            "note",
        ),
        rule_builder(
            "fallow/runtime-coverage-unavailable",
            "Runtime coverage could not be resolved for this function",
            "note",
        ),
        rule_builder(
            "fallow/runtime-coverage",
            "Runtime coverage finding",
            "note",
        ),
    ]
}

fn health_coverage_intelligence_sarif_rules(
    rule_builder: &SarifRuleBuilder<'_>,
) -> Vec<serde_json::Value> {
    vec![
        rule_builder(
            "fallow/coverage-intelligence-risky-change",
            "Changed hot path combines high CRAP and low test coverage",
            "warning",
        ),
        rule_builder(
            "fallow/coverage-intelligence-delete",
            "Static and runtime evidence indicate code can be deleted",
            "warning",
        ),
        rule_builder(
            "fallow/coverage-intelligence-review",
            "Cold reachable uncovered code needs owner review",
            "warning",
        ),
        rule_builder(
            "fallow/coverage-intelligence-refactor",
            "Hot covered code has high CRAP and should be refactored carefully",
            "warning",
        ),
    ]
}

fn append_complexity_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    report: &HealthReport,
    root: &Path,
    snippets: &mut SourceSnippetCache,
) {
    for finding in &report.findings {
        let uri = relative_uri(&finding.path, root);
        let (rule_id, message) = health_complexity_sarif_message(finding, report);
        let level = match finding.severity {
            FindingSeverity::Critical => "error",
            FindingSeverity::High => "warning",
            FindingSeverity::Moderate => "note",
        };
        let source_snippet = snippets.line(&finding.path, finding.line);
        sarif_results.push(sarif_result_with_snippet(
            rule_id,
            level,
            &message,
            &uri,
            Some((finding.line, finding.col + 1)),
            source_snippet.as_deref(),
        ));
    }
}

fn health_complexity_sarif_message(
    finding: &fallow_output::ComplexityViolation,
    report: &HealthReport,
) -> (&'static str, String) {
    match finding.exceeded {
        ExceededThreshold::Cyclomatic => (
            "fallow/high-cyclomatic-complexity",
            format!(
                "'{}' has cyclomatic complexity {} (threshold: {})",
                finding.name, finding.cyclomatic, report.summary.max_cyclomatic_threshold,
            ),
        ),
        ExceededThreshold::Cognitive => (
            "fallow/high-cognitive-complexity",
            format!(
                "'{}' has cognitive complexity {} (threshold: {})",
                finding.name, finding.cognitive, report.summary.max_cognitive_threshold,
            ),
        ),
        ExceededThreshold::Both => (
            "fallow/high-complexity",
            format!(
                "'{}' has cyclomatic complexity {} (threshold: {}) and cognitive complexity {} (threshold: {})",
                finding.name,
                finding.cyclomatic,
                report.summary.max_cyclomatic_threshold,
                finding.cognitive,
                report.summary.max_cognitive_threshold,
            ),
        ),
        ExceededThreshold::Crap
        | ExceededThreshold::CyclomaticCrap
        | ExceededThreshold::CognitiveCrap
        | ExceededThreshold::All => {
            let crap = finding.crap.unwrap_or(0.0);
            let coverage = finding
                .coverage_pct
                .map(|pct| format!(", coverage {pct:.0}%"))
                .unwrap_or_default();
            (
                "fallow/high-crap-score",
                format!(
                    "'{}' has CRAP score {:.1} (threshold: {:.1}, cyclomatic {}{})",
                    finding.name,
                    crap,
                    report.summary.max_crap_threshold,
                    finding.cyclomatic,
                    coverage,
                ),
            )
        }
    }
}

fn append_refactoring_target_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    report: &HealthReport,
    root: &Path,
) {
    for target in &report.targets {
        let uri = relative_uri(&target.path, root);
        let message = format!(
            "[{}] {} (priority: {:.1}, efficiency: {:.1}, effort: {}, confidence: {})",
            target.category.label(),
            target.recommendation,
            target.priority,
            target.efficiency,
            target.effort.label(),
            target.confidence.label(),
        );
        sarif_results.push(sarif_result(
            "fallow/refactoring-target",
            "warning",
            &message,
            &uri,
            None,
        ));
    }
}

fn append_coverage_gap_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    report: &HealthReport,
    root: &Path,
    snippets: &mut SourceSnippetCache,
) {
    let Some(ref gaps) = report.coverage_gaps else {
        return;
    };
    for item in &gaps.files {
        let uri = relative_uri(&item.file.path, root);
        let message = format!(
            "File is runtime-reachable but has no test dependency path ({} value export{})",
            item.file.value_export_count,
            if item.file.value_export_count == 1 {
                ""
            } else {
                "s"
            },
        );
        sarif_results.push(sarif_result(
            "fallow/untested-file",
            "warning",
            &message,
            &uri,
            None,
        ));
    }

    for item in &gaps.exports {
        let uri = relative_uri(&item.export.path, root);
        let message = format!(
            "Export '{}' is runtime-reachable but never referenced by test-reachable modules",
            item.export.export_name
        );
        let source_snippet = snippets.line(&item.export.path, item.export.line);
        sarif_results.push(sarif_result_with_snippet(
            "fallow/untested-export",
            "warning",
            &message,
            &uri,
            Some((item.export.line, item.export.col + 1)),
            source_snippet.as_deref(),
        ));
    }
}

fn append_runtime_coverage_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    production: &RuntimeCoverageReport,
    root: &Path,
    snippets: &mut SourceSnippetCache,
) {
    for finding in &production.findings {
        let uri = relative_uri(&finding.path, root);
        let rule_id = match finding.verdict {
            RuntimeCoverageVerdict::SafeToDelete => "fallow/runtime-safe-to-delete",
            RuntimeCoverageVerdict::ReviewRequired => "fallow/runtime-review-required",
            RuntimeCoverageVerdict::LowTraffic => "fallow/runtime-low-traffic",
            RuntimeCoverageVerdict::CoverageUnavailable => "fallow/runtime-coverage-unavailable",
            RuntimeCoverageVerdict::Active | RuntimeCoverageVerdict::Unknown => {
                "fallow/runtime-coverage"
            }
        };
        let level = match finding.verdict {
            RuntimeCoverageVerdict::SafeToDelete | RuntimeCoverageVerdict::ReviewRequired => {
                "warning"
            }
            _ => "note",
        };
        let invocations_hint = finding.invocations.map_or_else(
            || "untracked".to_owned(),
            |hits| format!("{hits} invocations"),
        );
        let message = format!(
            "'{}' runtime coverage verdict: {} ({})",
            finding.function,
            finding.verdict.human_label(),
            invocations_hint,
        );
        let source_snippet = snippets.line(&finding.path, finding.line);
        sarif_results.push(sarif_result_with_snippet(
            rule_id,
            level,
            &message,
            &uri,
            Some((finding.line, 1)),
            source_snippet.as_deref(),
        ));
    }
}

fn append_coverage_intelligence_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    intelligence: &CoverageIntelligenceReport,
    root: &Path,
    snippets: &mut SourceSnippetCache,
) {
    for finding in &intelligence.findings {
        let rule_id = coverage_intelligence_rule_id(finding.recommendation);
        let level = match finding.verdict {
            CoverageIntelligenceVerdict::Clean | CoverageIntelligenceVerdict::Unknown => continue,
            _ => "warning",
        };
        let uri = relative_uri(&finding.path, root);
        let identity = finding.identity.as_deref().unwrap_or("code");
        let signals = finding
            .signals
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let message = format!(
            "'{}' coverage intelligence verdict: {} ({}, signals: {})",
            identity, finding.verdict, finding.recommendation, signals,
        );
        let source_snippet = snippets.line(&finding.path, finding.line);
        let mut result = sarif_result_with_snippet(
            rule_id,
            level,
            &message,
            &uri,
            Some((finding.line, 1)),
            source_snippet.as_deref(),
        );
        result["properties"] = serde_json::json!({
            "coverage_intelligence_id": &finding.id,
            "verdict": finding.verdict,
            "recommendation": finding.recommendation,
            "confidence": finding.confidence,
            "signals": &finding.signals,
            "related_ids": &finding.related_ids,
        });
        sarif_results.push(result);
    }
}

fn coverage_intelligence_rule_id(
    recommendation: CoverageIntelligenceRecommendation,
) -> &'static str {
    match recommendation {
        CoverageIntelligenceRecommendation::AddTestOrSplitBeforeMerge => {
            "fallow/coverage-intelligence-risky-change"
        }
        CoverageIntelligenceRecommendation::DeleteAfterConfirmingOwner => {
            "fallow/coverage-intelligence-delete"
        }
        CoverageIntelligenceRecommendation::ReviewBeforeChanging => {
            "fallow/coverage-intelligence-review"
        }
        CoverageIntelligenceRecommendation::RefactorCarefullyKeepBehavior => {
            "fallow/coverage-intelligence-refactor"
        }
    }
}

fn sarif_document(
    sarif_results: &[serde_json::Value],
    sarif_rules: &[serde_json::Value],
) -> serde_json::Value {
    build_sarif_document(SarifDocumentInput {
        results: sarif_results,
        rules: sarif_rules,
        tool_version: env!("CARGO_PKG_VERSION"),
    })
}

fn sarif_result(
    rule_id: &str,
    level: &str,
    message: &str,
    uri: &str,
    region: Option<(u32, u32)>,
) -> serde_json::Value {
    sarif_result_with_snippet(rule_id, level, message, uri, region, None)
}

fn sarif_result_with_snippet(
    rule_id: &str,
    level: &str,
    message: &str,
    uri: &str,
    region: Option<(u32, u32)>,
    snippet: Option<&str>,
) -> serde_json::Value {
    build_sarif_result(SarifResultInput {
        rule_id,
        level,
        message,
        uri,
        region,
        snippet,
    })
}

fn relative_uri(path: &Path, root: &Path) -> String {
    normalize_uri(
        &path
            .strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_output::{SarifRuleInput, build_sarif_rule};
    use fallow_types::duplicates::{CloneGroup, CloneInstance, DuplicationStats};

    use super::*;

    fn rule(id: &str, short_description: &str, level: &str) -> serde_json::Value {
        build_sarif_rule(SarifRuleInput {
            id,
            short_description,
            level,
            full_description: None,
            help_uri: None,
        })
    }

    #[test]
    fn grouped_duplication_sarif_attaches_group_property() {
        let root = PathBuf::from("/repo");
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![CloneInstance {
                    file: root.join("src/a.ts"),
                    start_line: 2,
                    end_line: 5,
                    start_col: 0,
                    end_col: 1,
                    fragment: "copy();".to_string(),
                }],
                token_count: 10,
                line_count: 4,
            }],
            clone_families: Vec::new(),
            mirrored_directories: Vec::new(),
            stats: DuplicationStats::default(),
        };

        let sarif = build_grouped_duplication_sarif(&report, &root, &rule, |_| "src".to_string());

        assert_eq!(sarif["runs"][0]["results"][0]["properties"]["group"], "src");
        assert_eq!(
            sarif["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]
                ["uri"],
            "src/a.ts"
        );
    }
}
