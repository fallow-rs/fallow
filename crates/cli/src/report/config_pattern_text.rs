//! The text of the config entries that had no effect outside JSON and SARIF:
//! the unmatched patterns (`ignoreFindings`, `ignoreDependencies`), the
//! redundant entries (`entry`, `ignoreDependencies`, `ignorePatterns`) and the
//! `overrides` rules that cannot change a result.
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
/// `ignoreFindings`, `ignoreDependencies`, then one note line per config
/// setting with redundant entries, in the order `entry`,
/// `ignoreDependencies`, `ignorePatterns`, then one note line per `overrides`
/// rule that had no effect.
#[must_use]
pub fn stderr_notes(diagnostics: &[WorkspaceDiagnostic]) -> Vec<String> {
    let patterns_of = |kind: &str| -> Vec<&str> {
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind.id() == kind)
            .filter_map(|diagnostic| diagnostic.kind.unmatched_config_pattern())
            .map(|(_, pattern)| pattern)
            .collect()
    };
    let notes = [
        (
            "ignore-findings-pattern-unmatched",
            ("ignoreFindings pattern", "ignoreFindings patterns"),
            "matched no finding this run",
            "patterns are project-root-relative globs; check for typos",
        ),
        (
            "ignore-dependencies-glob-unmatched",
            ("ignoreDependencies glob", "ignoreDependencies globs"),
            "matched no declared dependency this run",
            "globs match package names such as @scope/*; check for typos",
        ),
        (
            "entry-pattern-redundant",
            ("entry pattern", "entry patterns"),
            "matched only files that are entry points anyway",
            "remove from the config",
        ),
        (
            "ignore-dependencies-entry-redundant",
            ("ignoreDependencies entry", "ignoreDependencies entries"),
            "hid no dependency finding this run",
            "remove from the config",
        ),
        (
            "ignore-patterns-entry-redundant",
            ("ignorePatterns entry", "ignorePatterns entries"),
            "excluded no file that fallow would otherwise analyze this run",
            "remove from the config; a glob for build output has an effect only after a build",
        ),
    ];
    let override_notes = diagnostics.iter().filter_map(|diagnostic| {
        let (rule, files, _) = diagnostic.kind.ineffective_override()?;
        let hint = diagnostic.kind.ineffective_override_hint()?;
        Some(format!(
            "Note: override rule had no effect: {rule} for [{}] ({hint}).",
            files.join(", ")
        ))
    });
    notes
        .into_iter()
        .filter_map(|(kind, (singular, plural), outcome, hint)| {
            let patterns = patterns_of(kind);
            if patterns.is_empty() {
                return None;
            }
            let noun = if patterns.len() == 1 {
                singular
            } else {
                plural
            };
            Some(format!(
                "Note: {noun} {outcome}: {} ({hint}).",
                patterns.join(", ")
            ))
        })
        .chain(override_notes)
        .collect()
}

/// The Markdown sections that list the unmatched patterns, the redundant
/// entries and the `overrides` rules with no effect, or `None` when there are
/// none. The text starts with a blank line,
/// so a caller appends it directly after the findings.
#[must_use]
pub fn markdown_section(diagnostics: &[WorkspaceDiagnostic]) -> Option<String> {
    let lines = |redundant: bool, outcome: &str| -> Vec<String> {
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind.is_redundant_config_entry() == redundant)
            .filter_map(|diagnostic| {
                let (setting, pattern) = diagnostic.kind.unmatched_config_pattern()?;
                Some(format!("- `{setting}`: `{pattern}` {outcome}"))
            })
            .collect()
    };
    let unmatched = lines(false, "matched nothing in this run");
    let redundant = lines(true, "had no effect in this run");
    let mut sections = Vec::new();
    if !unmatched.is_empty() {
        sections.push(format!(
            "\n## Unmatched config patterns\n\nThese entries had no effect. Fix a typo, or \
             remove the entry from the config.\n\n{}",
            unmatched.join("\n")
        ));
    }
    if !redundant.is_empty() {
        sections.push(format!(
            "\n## Redundant config entries\n\nThese entries matched, but fallow does the same \
             without them. Remove the entry from the config.\n\n{}",
            redundant.join("\n")
        ));
    }
    let overrides: Vec<String> = diagnostics
        .iter()
        .filter_map(|diagnostic| {
            let (rule, files, _) = diagnostic.kind.ineffective_override()?;
            let hint = diagnostic.kind.ineffective_override_hint()?;
            let globs = files
                .iter()
                .map(|glob| format!("`{glob}`"))
                .collect::<Vec<_>>()
                .join(", ");
            Some(format!("- `{rule}` for {globs}: {hint}"))
        })
        .collect();
    if !overrides.is_empty() {
        sections.push(format!(
            "\n## Override rules with no effect\n\nFallow ignored these `overrides` rules. Each \
             line names the mechanism that works.\n\n{}",
            overrides.join("\n")
        ));
    }
    (!sections.is_empty()).then(|| sections.join("\n"))
}

/// The config entries with no effect of a saved JSON envelope.
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
            .filter(|diagnostic| {
                diagnostic.kind.unmatched_config_pattern().is_some()
                    || diagnostic.kind.ineffective_override().is_some()
            });
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

    fn redundant_entries() -> Vec<WorkspaceDiagnostic> {
        vec![
            diagnostic(WorkspaceDiagnosticKind::IgnorePatternsEntryRedundant {
                pattern: "dist/**".to_owned(),
            }),
            diagnostic(WorkspaceDiagnosticKind::EntryPatternRedundant {
                pattern: "src/index.ts".to_owned(),
            }),
            diagnostic(WorkspaceDiagnosticKind::IgnoreDependenciesEntryRedundant {
                pattern: "left-pad".to_owned(),
            }),
            diagnostic(WorkspaceDiagnosticKind::IgnoreDependenciesEntryRedundant {
                pattern: "lodash".to_owned(),
            }),
        ]
    }

    #[test]
    fn stderr_notes_keep_unmatched_and_redundant_dependency_entries_apart() {
        let mut diagnostics = redundant_entries();
        diagnostics.push(glob("@typo/*"));
        let notes = stderr_notes(&diagnostics);
        assert_eq!(
            notes,
            vec![
                "Note: ignoreDependencies glob matched no declared dependency this run: @typo/* \
                 (globs match package names such as @scope/*; check for typos)."
                    .to_owned(),
                "Note: entry pattern matched only files that are entry points anyway: \
                 src/index.ts (remove from the config)."
                    .to_owned(),
                "Note: ignoreDependencies entries hid no dependency finding this run: left-pad, \
                 lodash (remove from the config)."
                    .to_owned(),
                "Note: ignorePatterns entry excluded no file that fallow would otherwise analyze \
                 this run: dist/** (remove from the config; a glob for build output has an \
                 effect only after a build)."
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn markdown_lists_redundant_entries_in_their_own_section() {
        let only_redundant = markdown_section(&redundant_entries()).expect("redundant entries");
        assert!(
            only_redundant.starts_with("\n## Redundant config entries\n"),
            "{only_redundant}"
        );
        assert!(!only_redundant.contains("Unmatched"), "{only_redundant}");
        assert!(
            only_redundant.contains("- `entry`: `src/index.ts` had no effect in this run"),
            "{only_redundant}"
        );

        let mut diagnostics = redundant_entries();
        diagnostics.push(glob("@typo/*"));
        let both = markdown_section(&diagnostics).expect("both sections");
        let unmatched = both
            .find("## Unmatched config patterns")
            .expect("unmatched section");
        let redundant = both
            .find("## Redundant config entries")
            .expect("redundant section");
        assert!(unmatched < redundant, "{both}");
        assert!(
            !both[unmatched..redundant].contains("left-pad"),
            "a redundant entry stays out of the unmatched section: {both}"
        );
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

    fn override_entry(rule: &str, files: &[&str], reason: &str) -> WorkspaceDiagnostic {
        diagnostic(WorkspaceDiagnosticKind::OverrideRuleIneffective {
            rule: rule.to_owned(),
            files: files.iter().map(|file| (*file).to_owned()).collect(),
            reason: reason.to_owned(),
        })
    }

    #[test]
    fn override_rules_get_one_note_per_entry_after_the_other_notes() {
        let notes = stderr_notes(&[
            override_entry("duplicate-exports", &["src/**", "lib/**"], "project-level"),
            override_entry("unused-files", &["gen/**"], "base-rule-off"),
            glob("@typo/*"),
        ]);
        assert_eq!(
            notes,
            vec![
                "Note: ignoreDependencies glob matched no declared dependency this run: @typo/* \
                 (globs match package names such as @scope/*; check for typos)."
                    .to_owned(),
                "Note: override rule had no effect: duplicate-exports for [src/**, lib/**] \
                 (project-level rule; add the file and export names to ignoreExports)."
                    .to_owned(),
                "Note: override rule had no effect: unused-files for [gen/**] (the top-level \
                 rule is off, so the detector does not run; set the top-level rule on, then use \
                 an override to set off for the other files)."
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn markdown_lists_override_rules_in_their_own_section() {
        let section = markdown_section(&[override_entry(
            "re-export-cycle",
            &["src/**", "lib/**"],
            "project-level",
        )])
        .expect("one override rule");
        assert!(
            section.starts_with("\n## Override rules with no effect\n"),
            "{section}"
        );
        assert!(
            section.contains(
                "- `re-export-cycle` for `src/**`, `lib/**`: project-level rule; add `// \
                 fallow-ignore-file re-export-cycle` to one file in the cycle, or set the \
                 top-level rule"
            ),
            "{section}"
        );
        assert!(!section.contains("Redundant"), "{section}");
        assert!(!section.contains("Unmatched"), "{section}");
    }

    #[test]
    fn envelope_diagnostics_read_override_rules() {
        let entry = override_entry("duplicate-exports", &["src/**"], "project-level");
        let entries = serde_json::to_value([&entry, &entry]).expect("serialize diagnostics");
        let envelope = serde_json::json!({ "workspace_diagnostics": entries });
        let read = envelope_diagnostics(&envelope);
        assert_eq!(read.len(), 1, "an equal entry is read once");
        assert_eq!(read[0].kind, entry.kind);
        assert_eq!(read[0].message, entry.message);
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
