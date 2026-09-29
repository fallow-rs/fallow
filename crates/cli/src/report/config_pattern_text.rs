//! The text of the unmatched config pattern entries (`ignoreFindings`,
//! `ignoreDependencies`) outside JSON and SARIF.
//!
//! Every output reads the same `workspace_diagnostics[]` entries, and each
//! format carries them in exactly one place:
//!
//! - JSON has the entries, SARIF has configuration notifications.
//! - Markdown, the job summary, the PR comment and the review summary body
//!   have a Markdown section.
//! - Every other format has a stderr note, so its stdout stays unchanged.
//!
//! A live run and `fallow report --from` use the same place for one format,
//! so a re-render of a saved envelope states what the live run stated.

use fallow_config::{OutputFormat, WorkspaceDiagnostic};

/// Whether `format` carries the entries in the document on stdout. A format
/// that does not gets a stderr note from [`print_stderr_notes`].
#[must_use]
pub const fn document_carries(format: OutputFormat) -> bool {
    matches!(
        format,
        OutputFormat::Json
            | OutputFormat::Sarif
            | OutputFormat::Markdown
            | OutputFormat::GithubSummary
            | OutputFormat::PrCommentGithub
            | OutputFormat::PrCommentGitlab
            | OutputFormat::ReviewGithub
            | OutputFormat::ReviewGitlab
    )
}

/// Print one stderr note per setting with unmatched patterns, unless `format`
/// carries the entries in its document or `quiet` is set.
pub fn print_stderr_notes(diagnostics: &[WorkspaceDiagnostic], format: OutputFormat, quiet: bool) {
    if quiet || document_carries(format) {
        return;
    }
    for note in stderr_notes(diagnostics) {
        eprintln!("{note}");
    }
}

/// One note line per config setting with unmatched patterns, in the order
/// `ignoreFindings`, `ignoreDependencies`.
#[must_use]
pub fn stderr_notes(diagnostics: &[WorkspaceDiagnostic]) -> Vec<String> {
    let patterns_of = |wanted: &str| -> Vec<&str> {
        diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic.kind.unmatched_config_pattern())
            .filter(|(setting, _)| *setting == wanted)
            .map(|(_, pattern)| pattern)
            .collect()
    };
    let mut notes = Vec::new();
    let findings = patterns_of("ignoreFindings");
    if !findings.is_empty() {
        let noun = if findings.len() == 1 {
            "pattern"
        } else {
            "patterns"
        };
        notes.push(format!(
            "Note: ignoreFindings {noun} matched no finding this run: {} (patterns are \
             project-root-relative globs; check for typos).",
            findings.join(", ")
        ));
    }
    let dependencies = patterns_of("ignoreDependencies");
    if !dependencies.is_empty() {
        let noun = if dependencies.len() == 1 {
            "glob"
        } else {
            "globs"
        };
        notes.push(format!(
            "Note: ignoreDependencies {noun} matched no declared dependency this run: {} \
             (globs match package names such as @scope/*; check for typos).",
            dependencies.join(", ")
        ));
    }
    notes
}

/// The Markdown section that lists the unmatched patterns, or `None` when no
/// pattern is unmatched. It starts with a blank line, so a caller appends it
/// directly after the findings.
#[must_use]
pub fn markdown_section(diagnostics: &[WorkspaceDiagnostic]) -> Option<String> {
    let lines = diagnostics
        .iter()
        .filter_map(|diagnostic| {
            let (setting, pattern) = diagnostic.kind.unmatched_config_pattern()?;
            Some(format!(
                "- `{setting}`: `{pattern}` matched nothing in this run"
            ))
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "\n## Unmatched config patterns\n\nThese entries had no effect. Fix a typo, or \
         remove the entry from the config.\n\n{}",
        lines.join("\n")
    ))
}

/// The unmatched config pattern entries of a saved JSON envelope.
///
/// The dead-code and combined envelopes keep the dead-code diagnostics in the
/// top-level `workspace_diagnostics[]`, the audit envelope keeps them in
/// `dead_code.workspace_diagnostics[]`. An entry this build cannot read (a
/// kind from a newer release) is skipped, not fatal. A pattern that shows at
/// two places is returned once.
#[must_use]
pub fn envelope_diagnostics(envelope: &serde_json::Value) -> Vec<WorkspaceDiagnostic> {
    let mut diagnostics: Vec<WorkspaceDiagnostic> = Vec::new();
    for pointer in ["/workspace_diagnostics", "/dead_code/workspace_diagnostics"] {
        let entries = envelope
            .pointer(pointer)
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                <WorkspaceDiagnostic as serde::Deserialize>::deserialize(entry).ok()
            })
            .filter(|diagnostic| diagnostic.kind.unmatched_config_pattern().is_some());
        for diagnostic in entries {
            if !diagnostics
                .iter()
                .any(|known| known.kind == diagnostic.kind)
            {
                diagnostics.push(diagnostic);
            }
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use fallow_config::{OutputFormat, WorkspaceDiagnostic, WorkspaceDiagnosticKind};

    use super::{document_carries, envelope_diagnostics, markdown_section, stderr_notes};

    fn diagnostic(kind: WorkspaceDiagnosticKind) -> WorkspaceDiagnostic {
        let root = std::path::Path::new("/project");
        WorkspaceDiagnostic::new(root, root.to_path_buf(), kind)
    }

    fn glob(pattern: &str) -> WorkspaceDiagnostic {
        diagnostic(WorkspaceDiagnosticKind::IgnoreDependenciesGlobUnmatched {
            pattern: pattern.to_owned(),
        })
    }

    fn finding_pattern(pattern: &str) -> WorkspaceDiagnostic {
        diagnostic(WorkspaceDiagnosticKind::IgnoreFindingsPatternUnmatched {
            pattern: pattern.to_owned(),
        })
    }

    #[test]
    fn stderr_notes_group_patterns_per_setting() {
        let notes = stderr_notes(&[
            glob("@a/*"),
            diagnostic(WorkspaceDiagnosticKind::NodeModulesMissing),
            glob("@b/*"),
            finding_pattern("src/legcy/**"),
        ]);
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(
            notes[0].starts_with(
                "Note: ignoreFindings pattern matched no finding this run: src/legcy/**"
            ),
            "{notes:?}"
        );
        assert!(
            notes[1].starts_with(
                "Note: ignoreDependencies globs matched no declared dependency this run: @a/*, @b/*"
            ),
            "{notes:?}"
        );
        assert!(stderr_notes(&[]).is_empty());
    }

    #[test]
    fn markdown_section_lists_each_unmatched_pattern() {
        let diagnostics = [
            glob("@typo/*"),
            diagnostic(WorkspaceDiagnosticKind::NodeModulesMissing),
        ];
        let section = markdown_section(&diagnostics).expect("one unmatched pattern");
        assert!(
            section.contains("## Unmatched config patterns"),
            "{section}"
        );
        assert!(
            section.contains("- `ignoreDependencies`: `@typo/*` matched nothing"),
            "{section}"
        );
        assert!(!section.contains("node_modules"), "{section}");
        assert!(markdown_section(&diagnostics[1..]).is_none());
    }

    #[test]
    fn each_format_has_one_carrier() {
        for format in [
            OutputFormat::Json,
            OutputFormat::Sarif,
            OutputFormat::Markdown,
            OutputFormat::GithubSummary,
            OutputFormat::PrCommentGithub,
            OutputFormat::PrCommentGitlab,
            OutputFormat::ReviewGithub,
            OutputFormat::ReviewGitlab,
        ] {
            assert!(document_carries(format), "{format:?}");
        }
        for format in [
            OutputFormat::Human,
            OutputFormat::Compact,
            OutputFormat::CodeClimate,
            OutputFormat::GithubAnnotations,
        ] {
            assert!(!document_carries(format), "{format:?}");
        }
    }

    #[test]
    fn envelope_diagnostics_read_every_envelope_shape_once() {
        let entries = serde_json::to_value([
            glob("@typo/*"),
            finding_pattern("src/legcy/**"),
            diagnostic(WorkspaceDiagnosticKind::NodeModulesMissing),
        ])
        .expect("serialize diagnostics");
        let unknown = serde_json::json!({"kind": "from-a-newer-release", "path": "."});
        for envelope in [
            serde_json::json!({ "workspace_diagnostics": entries }),
            serde_json::json!({ "dead_code": { "workspace_diagnostics": entries } }),
            serde_json::json!({
                "workspace_diagnostics": [unknown, entries[0]],
                "dead_code": { "workspace_diagnostics": entries },
            }),
        ] {
            let diagnostics = envelope_diagnostics(&envelope);
            let patterns = diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.kind.unmatched_config_pattern())
                .collect::<Vec<_>>();
            assert_eq!(
                patterns,
                vec![
                    ("ignoreDependencies", "@typo/*"),
                    ("ignoreFindings", "src/legcy/**")
                ],
                "{envelope}"
            );
        }
        assert!(envelope_diagnostics(&serde_json::json!({})).is_empty());
    }
}
