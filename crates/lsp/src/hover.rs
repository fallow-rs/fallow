use std::fmt::Write;
use std::path::Path;

use ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Range};

use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::{AnalysisResults, SecurityFindingKind};

use crate::diagnostics::security::security_label;
use crate::markdown::format_inline_code;

/// Build hover information for a position in a file.
///
/// Returns a hover with markdown content describing:
/// - Unused export/type status with explanation
/// - Used export reference counts with file locations
/// - Unused file status
/// - Unused member status
/// - Unresolved import details
/// - Code duplication instance details with other locations
pub fn build_hover(
    results: &AnalysisResults,
    duplication: &DuplicationReport,
    file_path: &Path,
    position: Position,
) -> Option<Hover> {
    if let Some(hover) = check_unused_file(results, file_path) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_export(results, file_path, position) {
        return Some(hover);
    }

    if let Some(hover) = check_used_export(results, file_path, position) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_member(results, file_path, position) {
        return Some(hover);
    }

    if let Some(hover) = check_unrendered_component(results, file_path, position) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_component_prop(results, file_path, position) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_component_emit(results, file_path, position) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_server_action(results, file_path, position) {
        return Some(hover);
    }

    if let Some(hover) = check_unresolved_import(results, file_path, position) {
        return Some(hover);
    }

    if let Some(hover) = check_security(results, file_path, position) {
        return Some(hover);
    }

    if let Some(hover) = check_duplication(duplication, file_path, position) {
        return Some(hover);
    }

    None
}

/// Check if the position is on a security candidate's anchor line.
///
/// The hover is a confidence-first TRIAGE surface, not a port of the CLI's
/// vertical report: it leads with the candidate kind + the honest confidence
/// signals (`source_backed`, `reachable_from_entry`), then evidence, then a
/// one-line blast-radius summary, the kind-appropriate next step, and a pointer
/// to the full trace (`fallow security --file`). The multi-hop traces stay out
/// of the hover. Every user-controlled string goes through `format_inline_code`
/// (never backslash-escaped) so a crafted evidence/path string cannot leak
/// markdown or a `command:` URI.
fn check_security(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
) -> Option<Hover> {
    for finding in &results.security_findings {
        if finding.path != file_path {
            continue;
        }
        let finding_line = finding.line.saturating_sub(1);
        if finding_line != position.line {
            continue;
        }
        if position.character < finding.col {
            continue;
        }

        let label = security_label(finding);
        let mut value = format!(
            "**fallow** security candidate: {} (unverified, verify before acting)",
            format_inline_code(&label),
        );

        let source_backed = if finding.source_backed { "yes" } else { "no" };
        let reachable = finding.reachability.as_ref().map_or("unknown", |r| {
            if r.reachable_from_entry { "yes" } else { "no" }
        });
        let _ = write!(
            value,
            "\n\nconfidence: source-backed {source_backed}, reachable from a runtime entry point \
             {reachable}",
        );

        let _ = write!(value, "\n\n{}", format_inline_code(&finding.evidence));

        if let Some(context) = finding.dead_code.as_ref() {
            // `guidance` is a trusted static constant from the analyzer
            // (`UNUSED_FILE_GUIDANCE` / `UNUSED_EXPORT_GUIDANCE` in
            // `analyze/security/rank.rs`), never user-derived, so it is rendered
            // as prose. If it ever becomes dynamic, route it through
            // `format_inline_code` or split out the user-controlled part.
            let _ = write!(value, "\n\ndead-code: {}", context.guidance);
        }

        if let Some(reach) = finding.reachability.as_ref() {
            let boundary = if reach.crosses_boundary {
                "; crosses an architecture boundary"
            } else {
                ""
            };
            let _ = write!(value, "\n\nblast radius {}{boundary}", reach.blast_radius);
        }

        let next = match finding.kind {
            SecurityFindingKind::ClientServerLeak => {
                "Next: check whether the import is type-only, server-only, or behind a build-time \
                 guard; if the value never ships to the client bundle, this candidate is a false \
                 positive."
            }
            SecurityFindingKind::TaintedSink if finding.dead_code.is_some() => {
                "Next: verify the dead-code finding and delete the code if safe; otherwise verify \
                 and harden the sink."
            }
            SecurityFindingKind::TaintedSink => {
                "Next: verify whether untrusted input can reach this sink; harden it or dismiss the \
                 candidate if it cannot."
            }
        };
        let _ = write!(value, "\n\n{next}");

        let basename = file_path.file_name().map_or_else(
            || file_path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let _ = write!(
            value,
            "\n\nFull trace: run {} or see the security docs.",
            format_inline_code(&format!("fallow security --file {basename}")),
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(Range {
                start: Position {
                    line: finding_line,
                    character: finding.col,
                },
                end: Position {
                    line: finding_line,
                    character: u32::MAX,
                },
            }),
        });
    }

    None
}

/// Check if the file is in the unused files list.
fn check_unused_file(results: &AnalysisResults, file_path: &Path) -> Option<Hover> {
    let is_unused = results
        .unused_files
        .iter()
        .any(|f| f.file.path == file_path);
    if !is_unused {
        return None;
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: "**fallow**: This file is not imported by any other file and is not reachable \
                    from any entry point."
                .to_string(),
        }),
        range: None,
    })
}

/// Check if the position is on an unused export or type.
#[expect(
    clippy::cast_possible_truncation,
    reason = "identifier lengths are bounded by source size"
)]
fn check_unused_export(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
) -> Option<Hover> {
    let unused_exports_iter = results.unused_exports.iter().map(|f| &f.export);
    let unused_types_iter = results.unused_types.iter().map(|f| &f.export);
    for (exports, kind_label) in [
        (
            Box::new(unused_exports_iter)
                as Box<dyn Iterator<Item = &fallow_core::results::UnusedExport>>,
            "Export",
        ),
        (
            Box::new(unused_types_iter)
                as Box<dyn Iterator<Item = &fallow_core::results::UnusedExport>>,
            "Type export",
        ),
    ] {
        for export in exports {
            if export.path != file_path {
                continue;
            }
            let export_line = export.line.saturating_sub(1);
            if export_line != position.line {
                continue;
            }
            let end_col = export.col + export.export_name.len() as u32;
            if position.character < export.col || position.character >= end_col {
                continue;
            }

            let value = format!(
                "**fallow**: {kind_label} {} is not imported by any other file.",
                format_inline_code(&export.export_name),
            );

            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value,
                }),
                range: Some(Range {
                    start: Position {
                        line: export_line,
                        character: export.col,
                    },
                    end: Position {
                        line: export_line,
                        character: export.col + export.export_name.len() as u32,
                    },
                }),
            });
        }
    }

    None
}

/// Check if the position is on a used export and show reference information.
#[expect(
    clippy::cast_possible_truncation,
    reason = "identifier lengths are bounded by source size"
)]
fn check_used_export(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
) -> Option<Hover> {
    for usage in &results.export_usages {
        if usage.path != file_path {
            continue;
        }
        let usage_line = usage.line.saturating_sub(1);
        if usage_line != position.line {
            continue;
        }
        let end_col = usage.col + usage.export_name.len() as u32;
        if position.character < usage.col || position.character >= end_col {
            continue;
        }

        if usage.reference_count == 0 {
            continue;
        }

        let ref_word = if usage.reference_count == 1 {
            "file"
        } else {
            "files"
        };

        let mut value = format!(
            "**fallow**: Export {} is used by {} {ref_word}",
            format_inline_code(&usage.export_name),
            usage.reference_count,
        );

        if usage.reference_locations.is_empty() {
            value.push('.');
        } else {
            value.push_str(":\n");
            for (i, loc) in usage.reference_locations.iter().take(10).enumerate() {
                let display_path = loc.path.file_name().map_or_else(
                    || loc.path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                let display_path = format_inline_code(&display_path);
                let _ = write!(value, "- {display_path} line {}", loc.line);
                if i < usage.reference_locations.len().min(10) - 1 {
                    value.push('\n');
                }
            }
            if usage.reference_locations.len() > 10 {
                let _ = write!(
                    value,
                    "\n- ... and {} more",
                    usage.reference_locations.len() - 10
                );
            }
        }

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(Range {
                start: Position {
                    line: usage_line,
                    character: usage.col,
                },
                end: Position {
                    line: usage_line,
                    character: usage.col + usage.export_name.len() as u32,
                },
            }),
        });
    }

    None
}

/// Check if the position is on an unused enum or class member.
#[expect(
    clippy::cast_possible_truncation,
    reason = "member name lengths are bounded by source size"
)]
fn check_unused_member(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
) -> Option<Hover> {
    let enum_iter = results.unused_enum_members.iter().map(|f| &f.member);
    let class_iter = results.unused_class_members.iter().map(|f| &f.member);
    let store_iter = results.unused_store_members.iter().map(|f| &f.member);
    for (members, kind_label) in [
        (
            Box::new(enum_iter) as Box<dyn Iterator<Item = &fallow_core::results::UnusedMember>>,
            "Enum member",
        ),
        (
            Box::new(class_iter) as Box<dyn Iterator<Item = &fallow_core::results::UnusedMember>>,
            "Class member",
        ),
        (
            Box::new(store_iter) as Box<dyn Iterator<Item = &fallow_core::results::UnusedMember>>,
            "Store member",
        ),
    ] {
        for member in members {
            if member.path != file_path {
                continue;
            }
            let member_line = member.line.saturating_sub(1);
            if member_line != position.line {
                continue;
            }
            let end_col = member.col + member.member_name.len() as u32;
            if position.character < member.col || position.character >= end_col {
                continue;
            }

            let qualified = format!("{}.{}", member.parent_name, member.member_name);
            let value = format!(
                "**fallow**: {kind_label} {} is never used outside its declaration.",
                format_inline_code(&qualified),
            );

            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value,
                }),
                range: Some(Range {
                    start: Position {
                        line: member_line,
                        character: member.col,
                    },
                    end: Position {
                        line: member_line,
                        character: member.col + member.member_name.len() as u32,
                    },
                }),
            });
        }
    }

    None
}

/// Check if the position is on an unrendered Vue/Svelte component anchor.
#[expect(
    clippy::cast_possible_truncation,
    reason = "component name lengths are bounded by source size"
)]
fn check_unrendered_component(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
) -> Option<Hover> {
    for finding in &results.unrendered_components {
        let c = &finding.component;
        if c.path != file_path {
            continue;
        }
        let component_line = c.line.saturating_sub(1);
        if component_line != position.line {
            continue;
        }
        let end_col = c.col + c.component_name.len() as u32;
        if position.character < c.col || position.character >= end_col {
            continue;
        }

        let value = format!(
            "**fallow**: Component {} is reachable but rendered nowhere in this project.",
            format_inline_code(&c.component_name),
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(Range {
                start: Position {
                    line: component_line,
                    character: c.col,
                },
                end: Position {
                    line: component_line,
                    character: end_col,
                },
            }),
        });
    }

    None
}

/// Check if the position is on an unused Vue component prop anchor.
#[expect(
    clippy::cast_possible_truncation,
    reason = "prop name lengths are bounded by source size"
)]
fn check_unused_component_prop(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
) -> Option<Hover> {
    for finding in &results.unused_component_props {
        let p = &finding.prop;
        if p.path != file_path {
            continue;
        }
        let prop_line = p.line.saturating_sub(1);
        if prop_line != position.line {
            continue;
        }
        let end_col = p.col + p.prop_name.len() as u32;
        if position.character < p.col || position.character >= end_col {
            continue;
        }

        let value = format!(
            "**fallow**: Prop {} is declared but referenced nowhere in this component.",
            format_inline_code(&p.prop_name),
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(Range {
                start: Position {
                    line: prop_line,
                    character: p.col,
                },
                end: Position {
                    line: prop_line,
                    character: end_col,
                },
            }),
        });
    }

    None
}

/// Check if the position is on an unused Vue component emit anchor.
#[expect(
    clippy::cast_possible_truncation,
    reason = "emit name lengths are bounded by source size"
)]
fn check_unused_component_emit(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
) -> Option<Hover> {
    for finding in &results.unused_component_emits {
        let e = &finding.emit;
        if e.path != file_path {
            continue;
        }
        let emit_line = e.line.saturating_sub(1);
        if emit_line != position.line {
            continue;
        }
        let end_col = e.col + e.emit_name.len() as u32;
        if position.character < e.col || position.character >= end_col {
            continue;
        }

        let value = format!(
            "**fallow**: Emit {} is declared but emitted nowhere in this component.",
            format_inline_code(&e.emit_name),
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(Range {
                start: Position {
                    line: emit_line,
                    character: e.col,
                },
                end: Position {
                    line: emit_line,
                    character: end_col,
                },
            }),
        });
    }

    None
}

/// Check if the position is on an unused Next.js server action.
#[expect(
    clippy::cast_possible_truncation,
    reason = "action name lengths are bounded by source size"
)]
fn check_unused_server_action(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
) -> Option<Hover> {
    for finding in &results.unused_server_actions {
        let a = &finding.action;
        if a.path != file_path {
            continue;
        }
        let action_line = a.line.saturating_sub(1);
        if action_line != position.line {
            continue;
        }
        let end_col = a.col + a.action_name.len() as u32;
        if position.character < a.col || position.character >= end_col {
            continue;
        }

        let value = format!(
            "**fallow**: Server action {} is exported from a \"use server\" file but no code in this project references it.",
            format_inline_code(&a.action_name),
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(Range {
                start: Position {
                    line: action_line,
                    character: a.col,
                },
                end: Position {
                    line: action_line,
                    character: end_col,
                },
            }),
        });
    }

    None
}

/// Check if the position is on an unresolved import.
#[expect(
    clippy::cast_possible_truncation,
    reason = "specifier lengths are bounded by source size"
)]
fn check_unresolved_import(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
) -> Option<Hover> {
    for import in &results.unresolved_imports {
        if import.import.path != file_path {
            continue;
        }
        let import_line = import.import.line.saturating_sub(1);
        if import_line != position.line {
            continue;
        }
        let end_col = import.import.specifier_col + import.import.specifier.len() as u32 + 2;
        if position.character < import.import.specifier_col || position.character >= end_col {
            continue;
        }

        let value = format!(
            "**fallow**: Cannot resolve import {}. The module may be missing, misspelled, \
             or not installed.",
            format_inline_code(&import.import.specifier),
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(Range {
                start: Position {
                    line: import_line,
                    character: import.import.specifier_col,
                },
                end: Position {
                    line: import_line,
                    character: end_col,
                },
            }),
        });
    }

    None
}

/// Check if the position overlaps with a code duplication instance.
#[expect(
    clippy::cast_possible_truncation,
    reason = "line/col numbers are bounded by source size"
)]
fn check_duplication(
    duplication: &DuplicationReport,
    file_path: &Path,
    position: Position,
) -> Option<Hover> {
    for group in &duplication.clone_groups {
        for instance in &group.instances {
            if instance.file != file_path {
                continue;
            }

            let start_line = (instance.start_line as u32).saturating_sub(1);
            let end_line = (instance.end_line as u32).saturating_sub(1);

            if position.line < start_line || position.line > end_line {
                continue;
            }

            let other_count = group.instances.len() - 1;
            let instance_word = if other_count == 1 {
                "instance"
            } else {
                "instances"
            };

            let mut value = format!(
                "**fallow**: Duplicated code block ({} lines, {} tokens). \
                 {other_count} other {instance_word}",
                group.line_count, group.token_count,
            );

            let others: Vec<_> = group
                .instances
                .iter()
                .filter(|other| {
                    !(other.file == instance.file && other.start_line == instance.start_line)
                })
                .collect();

            if others.is_empty() {
                value.push('.');
            } else {
                value.push_str(":\n");
                for (i, other) in others.iter().take(10).enumerate() {
                    let display_path = other.file.file_name().map_or_else(
                        || other.file.display().to_string(),
                        |name| name.to_string_lossy().into_owned(),
                    );
                    let display_path = format_inline_code(&display_path);
                    let _ = write!(
                        value,
                        "- {display_path} lines {}-{}",
                        other.start_line, other.end_line
                    );
                    if i < others.len().min(10) - 1 {
                        value.push('\n');
                    }
                }
                if others.len() > 10 {
                    let _ = write!(value, "\n- ... and {} more", others.len() - 10);
                }
            }

            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value,
                }),
                range: Some(Range {
                    start: Position {
                        line: start_line,
                        character: instance.start_col as u32,
                    },
                    end: Position {
                        line: end_line,
                        character: instance.end_col as u32,
                    },
                }),
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use fallow_core::duplicates::{CloneGroup, CloneInstance, DuplicationStats};
    use fallow_core::extract::MemberKind;
    use fallow_core::results::{
        ExportUsage, ReferenceLocation, SecuritySeverity, UnresolvedImport,
        UnresolvedImportFinding, UnusedClassMemberFinding, UnusedEnumMemberFinding, UnusedExport,
        UnusedExportFinding, UnusedFile, UnusedFileFinding, UnusedMember, UnusedStoreMemberFinding,
        UnusedTypeFinding,
    };

    /// Extract the markdown text from a Hover's contents.
    ///
    /// Panicking on an unexpected variant is acceptable in tests, but we use
    /// a descriptive assertion so the failure message is clearer than a bare
    /// `panic!`.
    fn markup_value(hover: &Hover) -> &str {
        match &hover.contents {
            HoverContents::Markup(m) => {
                assert_eq!(m.kind, MarkupKind::Markdown);
                &m.value
            }
            other => {
                panic!("Expected HoverContents::Markup, got {other:?}");
            }
        }
    }

    fn test_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\project")
        } else {
            PathBuf::from("/project")
        }
    }

    #[test]
    fn no_hover_for_clean_file() {
        let root = test_root();
        let path = root.join("src/clean.ts");
        let results = AnalysisResults::default();
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 5,
            character: 0,
        };

        let hover = build_hover(&results, &duplication, &path, pos);
        assert!(hover.is_none());
    }

    #[test]
    fn hover_on_unused_file() {
        let root = test_root();
        let path = root.join("src/dead.ts");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: path.clone(),
            }));
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 10,
            character: 0,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("not imported"));
        assert!(value.contains("entry point"));
    }

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test string lengths are trivially small"
    )]
    fn hover_on_unused_export() {
        let root = test_root();
        let path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "helper".to_string(),
                is_type_only: false,
                line: 5,
                col: 7,
                span_start: 40,
                is_re_export: false,
            }));
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 4, // 0-based
            character: 10,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("helper"));
        assert!(value.contains("not imported"));
        let range = hover.range.unwrap();
        assert_eq!(range.start.line, 4);
        assert_eq!(range.start.character, 7);
        assert_eq!(range.end.character, 7 + "helper".len() as u32);
    }

    #[test]
    fn hover_on_unused_type() {
        let root = test_root();
        let path = root.join("src/types.ts");
        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "MyType".to_string(),
                is_type_only: true,
                line: 3,
                col: 0,
                span_start: 20,
                is_re_export: false,
            }));
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 2, // 0-based
            character: 3,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("Type export"));
        assert!(value.contains("MyType"));
    }

    #[test]
    fn hover_on_used_export_with_references() {
        let root = test_root();
        let path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: path.clone(),
            export_name: "format".to_string(),
            line: 10,
            col: 7,
            reference_count: 2,
            reference_locations: vec![
                ReferenceLocation {
                    path: root.join("src/app.ts"),
                    line: 3,
                    col: 10,
                },
                ReferenceLocation {
                    path: root.join("src/main.ts"),
                    line: 8,
                    col: 0,
                },
            ],
        });
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 9, // 0-based
            character: 10,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("format"));
        assert!(value.contains("2 files"));
        assert!(value.contains("app.ts"));
        assert!(value.contains("main.ts"));
    }

    #[test]
    fn hover_on_used_export_single_reference() {
        let root = test_root();
        let path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: path.clone(),
            export_name: "helper".to_string(),
            line: 5,
            col: 0,
            reference_count: 1,
            reference_locations: vec![ReferenceLocation {
                path: root.join("src/app.ts"),
                line: 1,
                col: 0,
            }],
        });
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 4,
            character: 0,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("1 file"));
        assert!(!value.contains("1 files"));
    }

    #[test]
    fn hover_on_used_export_zero_refs_skipped() {
        let root = test_root();
        let path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: path.clone(),
            export_name: "unused".to_string(),
            line: 5,
            col: 0,
            reference_count: 0,
            reference_locations: vec![],
        });
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 4,
            character: 0,
        };

        let hover = build_hover(&results, &duplication, &path, pos);
        assert!(hover.is_none());
    }

    #[test]
    fn hover_on_unused_enum_member() {
        let root = test_root();
        let path = root.join("src/enums.ts");
        let mut results = AnalysisResults::default();
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: path.clone(),
                parent_name: "Color".to_string(),
                member_name: "Blue".to_string(),
                kind: MemberKind::EnumMember,
                line: 4,
                col: 2,
            }));
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 3,
            character: 5,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("Color.Blue"));
        assert!(value.contains("never used"));
    }

    #[test]
    fn hover_on_unused_class_member() {
        let root = test_root();
        let path = root.join("src/service.ts");
        let mut results = AnalysisResults::default();
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: path.clone(),
                parent_name: "UserService".to_string(),
                member_name: "reset".to_string(),
                kind: MemberKind::ClassMethod,
                line: 20,
                col: 4,
            }));
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 19,
            character: 6,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("UserService.reset"));
        assert!(value.contains("Class member"));
    }

    #[test]
    fn hover_on_unused_store_member() {
        let root = test_root();
        let path = root.join("src/store.ts");
        let mut results = AnalysisResults::default();
        results
            .unused_store_members
            .push(UnusedStoreMemberFinding::with_actions(UnusedMember {
                path: path.clone(),
                parent_name: "useStore".to_string(),
                member_name: "reset".to_string(),
                kind: MemberKind::StoreMember,
                line: 20,
                col: 4,
            }));
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 19,
            character: 6,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("useStore.reset"));
        assert!(value.contains("Store member"));
    }

    #[test]
    fn hover_on_unresolved_import() {
        let root = test_root();
        let path = root.join("src/app.ts");
        let mut results = AnalysisResults::default();
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: path.clone(),
                specifier: "./missing-module".to_string(),
                line: 3,
                col: 0,
                specifier_col: 20,
            }));
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 2,
            character: 25, // inside the specifier range [20, 38)
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("./missing-module"));
        assert!(value.contains("Cannot resolve"));
    }

    #[test]
    fn hover_on_duplication() {
        let root = test_root();
        let path_a = root.join("src/a.ts");
        let path_b = root.join("src/b.ts");
        let results = AnalysisResults::default();
        let duplication = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: path_a.clone(),
                        start_line: 10,
                        end_line: 15,
                        start_col: 0,
                        end_col: 20,
                        fragment: "duplicated code".to_string(),
                    },
                    CloneInstance {
                        file: path_b,
                        start_line: 20,
                        end_line: 25,
                        start_col: 4,
                        end_col: 24,
                        fragment: "duplicated code".to_string(),
                    },
                ],
                token_count: 50,
                line_count: 6,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 2,
                files_with_clones: 2,
                total_lines: 100,
                duplicated_lines: 12,
                total_tokens: 500,
                duplicated_tokens: 100,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 12.0,
                clone_groups_below_min_occurrences: 0,
            },
        };

        let pos = Position {
            line: 11, // Between lines 9 (0-based 10-1) and 14 (15-1)
            character: 5,
        };

        let hover = build_hover(&results, &duplication, &path_a, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("6 lines"));
        assert!(value.contains("50 tokens"));
        assert!(value.contains("1 other instance"));
        assert!(value.contains("b.ts"));

        let range = hover.range.unwrap();
        assert_eq!(range.start.line, 9); // 10 - 1
        assert_eq!(range.end.line, 14); // 15 - 1
    }

    #[test]
    fn hover_outside_duplication_range_returns_none() {
        let root = test_root();
        let path = root.join("src/a.ts");
        let results = AnalysisResults::default();
        let duplication = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![CloneInstance {
                    file: path.clone(),
                    start_line: 10,
                    end_line: 15,
                    start_col: 0,
                    end_col: 20,
                    fragment: "code".to_string(),
                }],
                token_count: 30,
                line_count: 6,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 1,
                files_with_clones: 1,
                total_lines: 50,
                duplicated_lines: 6,
                total_tokens: 200,
                duplicated_tokens: 30,
                clone_groups: 1,
                clone_instances: 1,
                duplication_percentage: 12.0,
                clone_groups_below_min_occurrences: 0,
            },
        };

        let pos = Position {
            line: 5,
            character: 0,
        };
        let hover = build_hover(&results, &duplication, &path, pos);
        assert!(hover.is_none());

        let pos = Position {
            line: 20,
            character: 0,
        };
        let hover = build_hover(&results, &duplication, &path, pos);
        assert!(hover.is_none());
    }

    #[test]
    fn unused_file_takes_priority_over_export_info() {
        let root = test_root();
        let path = root.join("src/dead.ts");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: path.clone(),
            }));
        results.export_usages.push(ExportUsage {
            path: path.clone(),
            export_name: "foo".to_string(),
            line: 5,
            col: 0,
            reference_count: 3,
            reference_locations: vec![],
        });
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 4,
            character: 0,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("not imported"));
        assert!(value.contains("entry point"));
    }

    #[test]
    fn hover_on_wrong_line_returns_none() {
        let root = test_root();
        let path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "helper".to_string(),
                is_type_only: false,
                line: 5,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let duplication = DuplicationReport::default();

        let pos = Position {
            line: 10,
            character: 0,
        };
        let hover = build_hover(&results, &duplication, &path, pos);
        assert!(hover.is_none());
    }

    #[test]
    fn hover_on_wrong_column_returns_none() {
        let root = test_root();
        let path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "helper".to_string(),
                is_type_only: false,
                line: 5,
                col: 7,
                span_start: 0,
                is_re_export: false,
            }));
        let duplication = DuplicationReport::default();

        let pos = Position {
            line: 4,
            character: 20,
        };
        let hover = build_hover(&results, &duplication, &path, pos);
        assert!(hover.is_none());

        let pos = Position {
            line: 4,
            character: 3,
        };
        let hover = build_hover(&results, &duplication, &path, pos);
        assert!(hover.is_none());
    }

    #[test]
    fn hover_duplication_multiple_instances() {
        let root = test_root();
        let path_a = root.join("src/a.ts");
        let path_b = root.join("src/b.ts");
        let path_c = root.join("src/c.ts");
        let results = AnalysisResults::default();
        let duplication = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: path_a.clone(),
                        start_line: 1,
                        end_line: 5,
                        start_col: 0,
                        end_col: 10,
                        fragment: "code".to_string(),
                    },
                    CloneInstance {
                        file: path_b,
                        start_line: 10,
                        end_line: 14,
                        start_col: 0,
                        end_col: 10,
                        fragment: "code".to_string(),
                    },
                    CloneInstance {
                        file: path_c,
                        start_line: 20,
                        end_line: 24,
                        start_col: 0,
                        end_col: 10,
                        fragment: "code".to_string(),
                    },
                ],
                token_count: 30,
                line_count: 5,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 3,
                files_with_clones: 3,
                total_lines: 100,
                duplicated_lines: 15,
                total_tokens: 500,
                duplicated_tokens: 90,
                clone_groups: 1,
                clone_instances: 3,
                duplication_percentage: 15.0,
                clone_groups_below_min_occurrences: 0,
            },
        };

        let pos = Position {
            line: 2,
            character: 0,
        };
        let hover = build_hover(&results, &duplication, &path_a, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("2 other instances"));
        assert!(value.contains("b.ts"));
        assert!(value.contains("c.ts"));
    }

    #[test]
    fn hover_on_used_export_no_locations_shows_period() {
        let root = test_root();
        let path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: path.clone(),
            export_name: "helper".to_string(),
            line: 5,
            col: 0,
            reference_count: 3,
            reference_locations: vec![], // no location details
        });
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 4,
            character: 0,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(
            value.ends_with('.'),
            "Expected message to end with period, got: {value}",
        );
        assert!(value.contains("3 files"));
        assert!(!value.contains('\n'));
    }

    #[test]
    fn hover_on_used_export_truncates_at_10_references() {
        let root = test_root();
        let path = root.join("src/popular.ts");
        let mut results = AnalysisResults::default();

        let locations: Vec<ReferenceLocation> = (1..=15)
            .map(|i| ReferenceLocation {
                path: root.join(format!("src/file{i}.ts")),
                line: i,
                col: 0,
            })
            .collect();

        results.export_usages.push(ExportUsage {
            path: path.clone(),
            export_name: "popular".to_string(),
            line: 1,
            col: 0,
            reference_count: 15,
            reference_locations: locations,
        });
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 0,
            character: 3,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("15 files"));
        for i in 1..=10 {
            assert!(
                value.contains(&format!("file{i}.ts")),
                "Expected file{i}.ts in hover, got: {value}",
            );
        }
        assert!(!value.contains("file11.ts"));
        assert!(
            value.contains("... and 5 more"),
            "Expected truncation message, got: {value}",
        );
    }

    #[test]
    fn hover_on_used_export_exactly_10_references_no_truncation() {
        let root = test_root();
        let path = root.join("src/moderate.ts");
        let mut results = AnalysisResults::default();

        let locations: Vec<ReferenceLocation> = (1..=10)
            .map(|i| ReferenceLocation {
                path: root.join(format!("src/ref{i}.ts")),
                line: i,
                col: 0,
            })
            .collect();

        results.export_usages.push(ExportUsage {
            path: path.clone(),
            export_name: "moderate".to_string(),
            line: 1,
            col: 0,
            reference_count: 10,
            reference_locations: locations,
        });
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 0,
            character: 0,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        for i in 1..=10 {
            assert!(value.contains(&format!("ref{i}.ts")));
        }
        assert!(!value.contains("... and"));
    }

    #[test]
    fn hover_on_unresolved_import_at_boundary_columns() {
        let root = test_root();
        let path = root.join("src/app.ts");
        let mut results = AnalysisResults::default();
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: path.clone(),
                specifier: "./mod".to_string(),
                line: 1,
                col: 0,
                specifier_col: 10,
            }));
        let duplication = DuplicationReport::default();

        let pos = Position {
            line: 0,
            character: 10,
        };
        assert!(build_hover(&results, &duplication, &path, pos).is_some());

        let pos = Position {
            line: 0,
            character: 16,
        };
        assert!(build_hover(&results, &duplication, &path, pos).is_some());

        let pos = Position {
            line: 0,
            character: 17,
        };
        assert!(build_hover(&results, &duplication, &path, pos).is_none());

        let pos = Position {
            line: 0,
            character: 9,
        };
        assert!(build_hover(&results, &duplication, &path, pos).is_none());
    }

    #[test]
    fn hover_on_unused_export_at_exact_boundary_columns() {
        let root = test_root();
        let path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "abc".to_string(),
                is_type_only: false,
                line: 1,
                col: 7,
                span_start: 0,
                is_re_export: false,
            }));
        let duplication = DuplicationReport::default();

        let pos = Position {
            line: 0,
            character: 7,
        };
        assert!(build_hover(&results, &duplication, &path, pos).is_some());

        let pos = Position {
            line: 0,
            character: 9,
        };
        assert!(build_hover(&results, &duplication, &path, pos).is_some());

        let pos = Position {
            line: 0,
            character: 10,
        };
        assert!(build_hover(&results, &duplication, &path, pos).is_none());
    }

    #[test]
    fn hover_on_unused_member_at_boundary_columns() {
        let root = test_root();
        let path = root.join("src/enums.ts");
        let mut results = AnalysisResults::default();
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: path.clone(),
                parent_name: "Color".to_string(),
                member_name: "Red".to_string(),
                kind: MemberKind::EnumMember,
                line: 3,
                col: 4,
            }));
        let duplication = DuplicationReport::default();

        let pos = Position {
            line: 2,
            character: 4,
        };
        assert!(build_hover(&results, &duplication, &path, pos).is_some());

        let pos = Position {
            line: 2,
            character: 7,
        };
        assert!(build_hover(&results, &duplication, &path, pos).is_none());
    }

    #[test]
    fn hover_duplication_with_more_than_10_other_instances() {
        let root = test_root();
        let path_main = root.join("src/main.ts");
        let results = AnalysisResults::default();

        let mut instances = vec![CloneInstance {
            file: path_main.clone(),
            start_line: 1,
            end_line: 5,
            start_col: 0,
            end_col: 10,
            fragment: "code".to_string(),
        }];
        for i in 1..=12 {
            instances.push(CloneInstance {
                file: root.join(format!("src/dup{i}.ts")),
                start_line: 10,
                end_line: 14,
                start_col: 0,
                end_col: 10,
                fragment: "code".to_string(),
            });
        }

        let duplication = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances,
                token_count: 30,
                line_count: 5,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats::default(),
        };

        let pos = Position {
            line: 2,
            character: 0,
        };
        let hover = build_hover(&results, &duplication, &path_main, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("12 other instances"));
        for i in 1..=10 {
            assert!(
                value.contains(&format!("dup{i}.ts")),
                "Expected dup{i}.ts in hover"
            );
        }
        assert!(!value.contains("dup11.ts"));
        assert!(value.contains("... and 2 more"));
    }

    #[test]
    fn hover_priority_unused_export_over_used_export() {
        let root = test_root();
        let path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();

        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "foo".to_string(),
                is_type_only: false,
                line: 5,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results.export_usages.push(ExportUsage {
            path: path.clone(),
            export_name: "foo".to_string(),
            line: 5,
            col: 0,
            reference_count: 2,
            reference_locations: vec![],
        });
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 4,
            character: 1,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("not imported"));
    }

    #[test]
    fn hover_on_unused_export_neutralizes_link_injection() {
        let root = test_root();
        let path = root.join("src/utils.ts");
        let crafted = "[click](command:vscode.open?evil)";
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: crafted.to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 0,
            character: 1,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);

        assert!(value.contains("`[click](command:vscode.open?evil)`"));
    }

    #[test]
    fn hover_on_unused_export_with_backtick_in_name_uses_escalated_fence() {
        let root = test_root();
        let path = root.join("src/utils.ts");
        let crafted = "evil`](command:foo)";
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: crafted.to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 0,
            character: 1,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);

        assert!(value.contains("``evil`](command:foo)``"));
        assert!(!value.contains("``](command:"));
    }

    #[test]
    fn hover_on_different_file_returns_none() {
        let root = test_root();
        let path_a = root.join("src/a.ts");
        let path_b = root.join("src/b.ts");

        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path_a,
                export_name: "foo".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let duplication = DuplicationReport::default();

        let pos = Position {
            line: 0,
            character: 0,
        };
        assert!(build_hover(&results, &duplication, &path_b, pos).is_none());
    }

    fn tainted_sink_finding(path: PathBuf) -> fallow_core::results::SecurityFinding {
        fallow_core::results::SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_core::results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: fallow_core::results::SecurityFindingKind::TaintedSink,
            category: Some("dangerous-html".to_string()),
            cwe: Some(79),
            path,
            line: 8,
            col: 6,
            evidence: "req.query.html flows into dangerouslySetInnerHTML".to_string(),
            source_backed: true,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![],
            actions: vec![],
            dead_code: None,
            reachability: Some(fallow_core::results::SecurityReachability {
                reachable_from_entry: true,
                reachable_from_untrusted_source: false,
                taint_confidence: None,
                untrusted_source_hop_count: None,
                untrusted_source_trace: vec![],
                blast_radius: 4,
                crosses_boundary: false,
            }),
            runtime: None,
        }
    }

    #[test]
    fn hover_on_security_candidate() {
        let root = test_root();
        let path = root.join("src/render.ts");
        let mut results = AnalysisResults::default();
        results
            .security_findings
            .push(tainted_sink_finding(path.clone()));
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 7, // 1-based 8 -> 0-based 7
            character: 10,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("security candidate"));
        assert!(value.contains("unverified"));
        assert!(value.contains("CWE-79"));
        assert!(value.contains("source-backed yes"));
        assert!(value.contains("reachable from a runtime entry point yes"));
        assert!(value.contains("dangerouslySetInnerHTML"));
        assert!(value.contains("blast radius 4"));
        assert!(value.contains("Next:"));
        assert!(value.contains("fallow security --file render.ts"));
        let range = hover.range.unwrap();
        assert_eq!(range.start.line, 7);
        assert_eq!(range.start.character, 6);
    }

    #[test]
    fn hover_off_security_candidate_line_returns_none() {
        let root = test_root();
        let path = root.join("src/render.ts");
        let mut results = AnalysisResults::default();
        results
            .security_findings
            .push(tainted_sink_finding(path.clone()));
        let duplication = DuplicationReport::default();

        // Wrong line.
        let pos = Position {
            line: 20,
            character: 6,
        };
        assert!(build_hover(&results, &duplication, &path, pos).is_none());

        // Before the anchor column.
        let pos = Position {
            line: 7,
            character: 2,
        };
        assert!(build_hover(&results, &duplication, &path, pos).is_none());
    }

    #[test]
    fn hover_on_security_candidate_neutralizes_link_injection() {
        let root = test_root();
        let path = root.join("src/render.ts");
        let mut finding = tainted_sink_finding(path.clone());
        finding.evidence = "[click](command:vscode.open?evil)".to_string();
        let mut results = AnalysisResults::default();
        results.security_findings.push(finding);
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 7,
            character: 6,
        };

        let hover = build_hover(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("`[click](command:vscode.open?evil)`"));
    }
}
