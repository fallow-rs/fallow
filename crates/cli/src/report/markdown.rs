use crate::report::sink::outln;
use std::path::Path;

use fallow_api::ResultGroup;
use fallow_types::duplicates::DuplicationReport;
use fallow_types::results::AnalysisResults;

use super::OwnershipResolver;
use super::github_annotations::EnvelopeKind;

pub(super) fn print_markdown(results: &AnalysisResults, root: &Path) {
    outln!("{}", fallow_api::build_markdown(results, root));
}

pub(super) fn print_grouped_markdown(groups: &[ResultGroup], root: &Path) {
    outln!("{}", fallow_api::build_grouped_markdown(groups, root));
}

pub(super) fn print_duplication_markdown(report: &DuplicationReport, root: &Path) {
    outln!("{}", fallow_api::build_duplication_markdown(report, root));
}

pub(super) fn print_health_markdown(report: &fallow_output::HealthReport, root: &Path) {
    outln!("{}", fallow_api::build_health_markdown(report, root));
}

/// Print the `## Health by <mode>` section of a grouped health run.
///
/// Renders through the same JSON-shaped renderer as the GitHub job summary,
/// so the two tables stay identical.
pub(super) fn print_health_grouping_markdown(
    grouping: &fallow_output::HealthGrouping,
    root: &Path,
) {
    let envelope = serde_json::json!({
        "grouped_by": grouping.mode,
        "groups": grouping.groups,
        "group_filter": grouping.filter,
    });
    let section = fallow_api::build_health_groups_markdown(&envelope, &root.to_string_lossy());
    if !section.is_empty() {
        outln!("{section}");
    }
}

pub(super) fn print_type_aware_markdown(
    type_aware: Option<&fallow_types::envelope::TypeAwareMeta>,
    scope: Option<&str>,
) {
    let Some(meta) = type_aware else {
        return;
    };
    let completeness = meta
        .identity
        .as_ref()
        .and_then(|identity| serde_json::to_value(identity.completeness).ok())
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_string());
    let capabilities = meta.identity.as_ref().map_or_else(
        || "none".to_string(),
        |identity| {
            identity
                .capabilities
                .iter()
                .filter_map(|capability| serde_json::to_value(capability).ok())
                .filter_map(|value| value.as_str().map(|label| format!("`{label}`")))
                .collect::<Vec<_>>()
                .join(", ")
        },
    );
    let heading = type_aware_heading(scope);
    outln!(
        "\n## {heading}\n\n- Executed: `{}`\n- Backend: `{}@{}`\n- Completeness: `{}`\n- Capabilities: {}\n- Confirmed used: {}\n- Contract preserved: {}\n- No static references: {}\n- Guarded fixes available: {}\n- Unresolved: {}\n- Abstained: {}\n- Warnings: {}",
        meta.executed,
        meta.backend,
        meta.backend_version.as_deref().unwrap_or("not-run"),
        completeness,
        capabilities,
        meta.confirmed_used_count,
        meta.contract_preserved_count,
        meta.no_static_references_count,
        meta.fix_eligible_count,
        meta.unresolved_count,
        meta.abstained_count,
        meta.warning_count
    );
}

/// Append the config patterns that matched nothing (`ignoreDependencies`,
/// `ignoreFindings`) as a section after the findings. The section is omitted
/// when no pattern is unmatched.
pub(super) fn print_config_pattern_markdown(diagnostics: &[fallow_config::WorkspaceDiagnostic]) {
    if let Some(section) = super::config_pattern_text::markdown_section(diagnostics) {
        outln!("{section}");
    }
}

/// Print the markdown document of a saved `--format json` envelope.
///
/// The document is byte-identical to the live `--format markdown` run that
/// produced the envelope, because both runs call the same section printers in
/// the same order. Every section is parsed before the first line prints, so a
/// malformed envelope prints nothing to stdout.
///
/// `resolver` is set exactly when the saved dead-code envelope was grouped.
///
/// # Errors
///
/// Returns the reason when the envelope kind has no saved markdown document,
/// or when a section does not parse with this Fallow version.
pub fn print_saved_markdown(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
    resolver: Option<&OwnershipResolver>,
) -> Result<(), String> {
    let document = SavedMarkdown::parse(kind, envelope, root, resolver)?;
    document.print(root);
    Ok(())
}

/// The sections of a saved envelope that its markdown document reads.
enum SavedMarkdown {
    DeadCode(SavedDeadCode),
    Dupes(DuplicationReport),
    Health(SavedHealth),
    Combined {
        check: Option<SavedDeadCode>,
        dupes: Option<DuplicationReport>,
        health: Option<Box<fallow_output::HealthReport>>,
    },
}

/// The dead-code section: the findings, the groups of a grouped run, the
/// type-aware evidence and the config patterns that matched nothing.
struct SavedDeadCode {
    results: AnalysisResults,
    groups: Option<Vec<ResultGroup>>,
    type_aware: Option<fallow_types::envelope::TypeAwareMeta>,
    type_aware_scope: Option<&'static str>,
    diagnostics: Vec<fallow_config::WorkspaceDiagnostic>,
}

/// The health section: the report, the `## Health by <mode>` section of a
/// grouped run, and the type-aware evidence.
struct SavedHealth {
    report: Box<fallow_output::HealthReport>,
    groups: String,
    type_aware: Option<fallow_types::envelope::TypeAwareMeta>,
}

impl SavedMarkdown {
    fn parse(
        kind: EnvelopeKind,
        envelope: &serde_json::Value,
        root: &Path,
        resolver: Option<&OwnershipResolver>,
    ) -> Result<Self, String> {
        match kind {
            EnvelopeKind::DeadCode => Ok(Self::DeadCode(SavedDeadCode::parse(
                envelope, envelope, root, resolver, None,
            )?)),
            EnvelopeKind::Dupes => Ok(Self::Dupes(parse_saved(envelope, "dupes envelope")?)),
            EnvelopeKind::Health => Ok(Self::Health(SavedHealth {
                report: Box::new(parse_saved(envelope, "health envelope")?),
                groups: fallow_api::build_health_groups_markdown(envelope, ""),
                type_aware: saved_type_aware(envelope)?,
            })),
            EnvelopeKind::Combined => parse_saved_combined(envelope, root),
            EnvelopeKind::Audit | EnvelopeKind::Security | EnvelopeKind::Fix => {
                Err(saved_markdown_unsupported(kind))
            }
        }
    }

    fn print(&self, root: &Path) {
        match self {
            Self::DeadCode(check) => check.print(root),
            Self::Dupes(report) => print_duplication_markdown(report, root),
            Self::Health(health) => {
                print_health_markdown(&health.report, root);
                if !health.groups.is_empty() {
                    outln!("{}", health.groups);
                }
                print_type_aware_markdown(health.type_aware.as_ref(), None);
            }
            Self::Combined {
                check,
                dupes,
                health,
            } => {
                if let Some(check) = check {
                    check.print(root);
                }
                if let Some(report) = dupes {
                    print_duplication_markdown(report, root);
                }
                if let Some(report) = health {
                    print_health_markdown(report, root);
                }
            }
        }
    }
}

impl SavedDeadCode {
    fn parse(
        section: &serde_json::Value,
        envelope: &serde_json::Value,
        root: &Path,
        resolver: Option<&OwnershipResolver>,
        type_aware_scope: Option<&'static str>,
    ) -> Result<Self, String> {
        let mut results: AnalysisResults = parse_saved(section, "dead-code envelope")?;
        // A grouped envelope is flattened in group order. The live render
        // reads the findings in the order of the analysis.
        results.sort();
        let groups = resolver
            .map(|resolver| super::grouping::group_analysis_results(&results, root, resolver));
        Ok(Self {
            results,
            groups,
            type_aware: saved_type_aware(envelope)?,
            type_aware_scope,
            diagnostics: super::config_pattern_text::envelope_diagnostics(envelope),
        })
    }

    fn print(&self, root: &Path) {
        match &self.groups {
            Some(groups) => print_grouped_markdown(groups, root),
            None => print_markdown(&self.results, root),
        }
        print_type_aware_markdown(self.type_aware.as_ref(), self.type_aware_scope);
        print_config_pattern_markdown(&self.diagnostics);
    }
}

/// The bare combined run. Its envelope keeps the type-aware evidence of the
/// dead-code section only, while the live markdown also prints the evidence
/// of the health section. An envelope with type-aware evidence is therefore
/// refused.
fn parse_saved_combined(
    envelope: &serde_json::Value,
    root: &Path,
) -> Result<SavedMarkdown, String> {
    if !super::ci::saved_type_aware_metadata(envelope)?.is_empty() {
        return Err(
            "saved combined envelopes with type-aware evidence do not support --format markdown; \
             render the dead-code and health envelopes instead"
                .to_owned(),
        );
    }
    let check = present_section(envelope, "/check")
        .map(|section| SavedDeadCode::parse(section, envelope, root, None, Some("dead-code")))
        .transpose()?;
    let dupes = present_section(envelope, "/dupes")
        .map(|section| parse_saved(section, "section `/dupes`"))
        .transpose()?;
    let health = present_section(envelope, "/health")
        .map(|section| parse_saved(section, "section `/health`").map(Box::new))
        .transpose()?;
    Ok(SavedMarkdown::Combined {
        check,
        dupes,
        health,
    })
}

fn present_section<'a>(
    envelope: &'a serde_json::Value,
    pointer: &str,
) -> Option<&'a serde_json::Value> {
    envelope.pointer(pointer).filter(|value| !value.is_null())
}

/// The type-aware evidence of a single-analysis envelope.
fn saved_type_aware(
    envelope: &serde_json::Value,
) -> Result<Option<fallow_types::envelope::TypeAwareMeta>, String> {
    present_section(envelope, "/_meta/type_aware")
        .map(|value| parse_saved(value, "type-aware metadata"))
        .transpose()
}

fn parse_saved<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    label: &str,
) -> Result<T, String> {
    serde_json::from_value(value.clone())
        .map_err(|error| format!("saved {label} is incompatible with this Fallow version: {error}"))
}

/// The refusal for an envelope kind without a saved markdown document.
///
/// The live `audit` markdown is the human report with markdown sections, and
/// `security` and `fix` print no markdown document.
pub fn saved_markdown_unsupported(kind: EnvelopeKind) -> String {
    let label = match kind {
        EnvelopeKind::Audit => "audit",
        EnvelopeKind::Security => "security",
        EnvelopeKind::Fix => "fix",
        EnvelopeKind::DeadCode => "dead-code",
        EnvelopeKind::Dupes => "dupes",
        EnvelopeKind::Health => "health",
        EnvelopeKind::Combined => "combined",
    };
    format!("saved {label} envelopes do not support --format markdown")
}

fn type_aware_heading(scope: Option<&str>) -> String {
    scope.map_or_else(
        || "Type-aware evidence".to_string(),
        |scope| format!("Type-aware {scope} evidence"),
    )
}

#[cfg(test)]
mod tests {
    use super::type_aware_heading;

    #[test]
    fn type_aware_heading_labels_combined_scope() {
        assert_eq!(
            type_aware_heading(Some("dead-code")),
            "Type-aware dead-code evidence"
        );
        assert_eq!(
            type_aware_heading(Some("health")),
            "Type-aware health evidence"
        );
    }
}
