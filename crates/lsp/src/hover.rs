use std::fmt::Write;
use std::path::Path;

use ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Range};

use fallow_api::editor_results::SecurityFindingKind;
use fallow_api::{
    EditorAnalysisResults as AnalysisResults, EditorDuplicationReport as DuplicationReport,
};

use crate::code_lens::{pluralize, react_hook_segment};
use crate::diagnostics::security::security_label;
use crate::markdown::format_inline_code;
use crate::position::{NamedAnchor, PositionMapper, line_range_from_byte_col};

/// Typed input for building hover information from editor analysis state.
#[derive(Clone, Copy)]
pub struct HoverInput<'a> {
    pub results: &'a AnalysisResults,
    pub duplication: &'a DuplicationReport,
    pub file_path: &'a Path,
    pub position: Position,
}

impl<'a> HoverInput<'a> {
    #[must_use]
    pub const fn new(
        results: &'a AnalysisResults,
        duplication: &'a DuplicationReport,
        file_path: &'a Path,
        position: Position,
    ) -> Self {
        Self {
            results,
            duplication,
            file_path,
            position,
        }
    }
}

/// Build hover information from a typed editor analysis input.
///
/// Returns a hover with markdown content describing unused code, unresolved
/// imports, security candidates, React component intelligence, or duplication
/// evidence for the requested file position.
pub fn build_hover(input: HoverInput<'_>) -> Option<Hover> {
    let HoverInput {
        results,
        duplication,
        file_path,
        position,
    } = input;
    let mut mapper = PositionMapper::default();
    if let Some(hover) = check_unused_file(results, file_path) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_export(results, file_path, position, &mut mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_used_export(results, file_path, position, &mut mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_member(results, file_path, position, &mut mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_component_hover(results, file_path, position, &mut mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_unresolved_import(results, file_path, position, &mut mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_security(results, file_path, position, &mut mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_duplication(duplication, file_path, position, &mut mapper) {
        return Some(hover);
    }

    None
}

fn check_component_hover(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    if let Some(hover) = check_unrendered_component(results, file_path, position, mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_component_prop(results, file_path, position, mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_component_emit(results, file_path, position, mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_component_input(results, file_path, position, mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_component_output(results, file_path, position, mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_svelte_event(results, file_path, position, mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_effect_finding(results, file_path, position, mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_server_action(results, file_path, position, mapper) {
        return Some(hover);
    }

    if let Some(hover) = check_unused_load_data_key(results, file_path, position, mapper) {
        return Some(hover);
    }

    // Descriptive React component intelligence (per-prop usage hover, plus a
    // component-anchor summary hover). Placed AFTER every component-finding
    // hover so a real finding (e.g. an unused prop) wins when both match.
    if let Some(hover) = check_react_prop_intel(results, file_path, position, mapper) {
        return Some(hover);
    }

    check_react_component_intel(results, file_path, position, mapper)
}

fn position_in_span(position: Position, start: u32, end: u32) -> bool {
    position.character >= start && position.character < end
}

fn span_range(line: u32, start: u32, end: u32) -> Range {
    Range {
        start: Position {
            line,
            character: start,
        },
        end: Position {
            line,
            character: end,
        },
    }
}

#[cfg(test)]
fn build_hover_for_test(
    results: &AnalysisResults,
    duplication: &DuplicationReport,
    file_path: &Path,
    position: Position,
) -> Option<Hover> {
    build_hover(HoverInput::new(results, duplication, file_path, position))
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
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    for finding in &results.security_findings {
        if finding.path != file_path {
            continue;
        }
        let finding_line = finding.line.saturating_sub(1);
        if finding_line != position.line {
            continue;
        }
        let range = line_range_from_byte_col(mapper, &finding.path, finding_line, finding.col);
        if position.character < range.start.character {
            continue;
        }

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: security_hover_markdown(finding, file_path),
            }),
            range: Some(range),
        });
    }

    None
}

/// Build the confidence-first triage markdown body for a security candidate.
fn security_hover_markdown(
    finding: &fallow_api::editor_results::SecurityFinding,
    file_path: &Path,
) -> String {
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

    let _ = write!(value, "\n\n{}", security_next_step(finding));

    let basename = file_path.file_name().map_or_else(
        || file_path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let _ = write!(
        value,
        "\n\nFull trace: run {} or see the security docs.",
        format_inline_code(&format!("fallow security --file {basename}")),
    );

    value
}

/// Kind-appropriate "Next:" guidance line for a security candidate.
fn security_next_step(finding: &fallow_api::editor_results::SecurityFinding) -> &'static str {
    match finding.kind {
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
    }
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
fn check_unused_export(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    let unused_exports_iter = results.unused_exports.iter().map(|f| &f.export);
    let unused_types_iter = results.unused_types.iter().map(|f| &f.export);
    for (exports, kind_label) in [
        (
            Box::new(unused_exports_iter)
                as Box<dyn Iterator<Item = &fallow_api::editor_results::UnusedExport>>,
            "Export",
        ),
        (
            Box::new(unused_types_iter)
                as Box<dyn Iterator<Item = &fallow_api::editor_results::UnusedExport>>,
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
            let (start_col, end_col) =
                mapper.utf16_col_span(&export.path, export_line, export.col, &export.export_name);
            if !position_in_span(position, start_col, end_col) {
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
                range: Some(span_range(export_line, start_col, end_col)),
            });
        }
    }

    None
}

/// Check if the position is on a used export and show reference information.
fn check_used_export(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    for usage in &results.export_usages {
        if usage.path != file_path {
            continue;
        }
        let usage_line = usage.line.saturating_sub(1);
        if usage_line != position.line {
            continue;
        }
        let (start_col, end_col) =
            mapper.utf16_col_span(&usage.path, usage_line, usage.col, &usage.export_name);
        if !position_in_span(position, start_col, end_col) {
            continue;
        }

        if usage.reference_count == 0 {
            continue;
        }

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: used_export_hover_markdown(usage),
            }),
            range: Some(span_range(usage_line, start_col, end_col)),
        });
    }

    None
}

/// Build the reference-count markdown body for a used export, listing up to
/// ten reference locations and a "... and N more" overflow line.
fn used_export_hover_markdown(usage: &fallow_api::editor_results::ExportUsage) -> String {
    let file_word = if usage.reference_count == 1 {
        "file"
    } else {
        "files"
    };

    let mut value = format!(
        "**fallow**: Export {} is imported by {} {file_word}",
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

    value
}

/// Check if the position is on an unused enum or class member.
fn check_unused_member(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    let enum_iter = results.unused_enum_members.iter().map(|f| &f.member);
    let class_iter = results.unused_class_members.iter().map(|f| &f.member);
    let store_iter = results.unused_store_members.iter().map(|f| &f.member);
    let namespace_iter = results.unused_namespace_members.iter().map(|f| &f.member);
    for (members, kind_label) in [
        (
            Box::new(enum_iter)
                as Box<dyn Iterator<Item = &fallow_api::editor_results::UnusedMember>>,
            "Enum member",
        ),
        (
            Box::new(class_iter)
                as Box<dyn Iterator<Item = &fallow_api::editor_results::UnusedMember>>,
            "Class member",
        ),
        (
            Box::new(store_iter)
                as Box<dyn Iterator<Item = &fallow_api::editor_results::UnusedMember>>,
            "Store member",
        ),
        (
            Box::new(namespace_iter)
                as Box<dyn Iterator<Item = &fallow_api::editor_results::UnusedMember>>,
            "Namespace member",
        ),
    ] {
        for member in members {
            if let Some(hover) =
                unused_member_hover(member, kind_label, file_path, position, mapper)
            {
                return Some(hover);
            }
        }
    }

    None
}

fn unused_member_hover(
    member: &fallow_api::editor_results::UnusedMember,
    kind_label: &str,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    if member.path != file_path {
        return None;
    }
    let member_line = member.line.saturating_sub(1);
    if member_line != position.line {
        return None;
    }
    let (start_col, end_col) =
        mapper.utf16_col_span(&member.path, member_line, member.col, &member.member_name);
    if !position_in_span(position, start_col, end_col) {
        return None;
    }

    let qualified = format!("{}.{}", member.parent_name, member.member_name);
    let value = format!(
        "**fallow**: {kind_label} {} is never used outside its declaration.",
        format_inline_code(&qualified),
    );
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(span_range(member_line, start_col, end_col)),
    })
}

/// Build the hover for a named anchor when the cursor is on its identifier.
fn anchor_hover(
    anchor: &NamedAnchor<'_>,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
    message: impl FnOnce() -> String,
) -> Option<Hover> {
    let line = anchor.line.saturating_sub(1);
    if anchor.path != file_path || line != position.line {
        return None;
    }
    let (start_col, end_col) = mapper.utf16_col_span(anchor.path, line, anchor.col, anchor.name);
    if !position_in_span(position, start_col, end_col) {
        return None;
    }
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: message(),
        }),
        range: Some(span_range(line, start_col, end_col)),
    })
}

/// Check if the position is on an unrendered Vue/Svelte component anchor.
fn check_unrendered_component(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    results.unrendered_components.iter().find_map(|finding| {
        let c = &finding.component;
        let anchor = NamedAnchor {
            path: &c.path,
            line: c.line,
            col: c.col,
            name: &c.component_name,
        };
        anchor_hover(&anchor, file_path, position, mapper, || {
            // Lit: `component_name` is the registered TAG; render it as a custom
            // element to match the CLI human / markdown formatters.
            if c.framework == "lit" {
                format!(
                    "**fallow**: Custom element {} is registered but rendered in no template.",
                    format_inline_code(&format!("<{}>", c.component_name)),
                )
            } else {
                format!(
                    "**fallow**: Component {} is reachable but rendered nowhere in this project.",
                    format_inline_code(&c.component_name),
                )
            }
        })
    })
}

/// Check if the position is on an unused component prop anchor.
fn check_unused_component_prop(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    results.unused_component_props.iter().find_map(|finding| {
        let p = &finding.prop;
        let anchor = NamedAnchor {
            path: &p.path,
            line: p.line,
            col: p.col,
            name: &p.prop_name,
        };
        anchor_hover(&anchor, file_path, position, mapper, || {
            format!(
                "**fallow**: Prop {} is declared but referenced nowhere in this component.",
                format_inline_code(&p.prop_name),
            )
        })
    })
}

/// Check if the position is on an unused Vue component emit anchor.
fn check_unused_component_emit(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    results.unused_component_emits.iter().find_map(|finding| {
        let e = &finding.emit;
        let anchor = NamedAnchor {
            path: &e.path,
            line: e.line,
            col: e.col,
            name: &e.emit_name,
        };
        anchor_hover(&anchor, file_path, position, mapper, || {
            format!(
                "**fallow**: Emit {} is declared but emitted nowhere in this component.",
                format_inline_code(&e.emit_name),
            )
        })
    })
}

/// Check if the position is on an unused Angular component input anchor.
fn check_unused_component_input(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    results.unused_component_inputs.iter().find_map(|finding| {
        let i = &finding.input;
        let anchor = NamedAnchor {
            path: &i.path,
            line: i.line,
            col: i.col,
            name: &i.input_name,
        };
        anchor_hover(&anchor, file_path, position, mapper, || {
            format!(
                "**fallow**: Input {} is declared but read nowhere in this component.",
                format_inline_code(&i.input_name),
            )
        })
    })
}

/// Check if the position is on an unused Angular component output anchor.
fn check_unused_component_output(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    results.unused_component_outputs.iter().find_map(|finding| {
        let o = &finding.output;
        let anchor = NamedAnchor {
            path: &o.path,
            line: o.line,
            col: o.col,
            name: &o.output_name,
        };
        anchor_hover(&anchor, file_path, position, mapper, || {
            format!(
                "**fallow**: Output {} is declared but emitted nowhere in this component.",
                format_inline_code(&o.output_name),
            )
        })
    })
}

/// Check if the position is on an Effect finding anchor: an unused layer, an
/// unused tagged error, or an unproduced `catchTag` handler.
fn check_effect_finding(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    let layer_hover = results.unused_effect_layers.iter().find_map(|finding| {
        let layer = &finding.layer;
        let anchor = NamedAnchor {
            path: &layer.path,
            line: layer.line,
            col: layer.col,
            name: layer.name.rsplit('.').next().unwrap_or(&layer.name),
        };
        anchor_hover(&anchor, file_path, position, mapper, || {
            format!(
                "**fallow**: Layer {} is composed only into layers that nothing uses: {}.",
                format_inline_code(&layer.name),
                layer.dead_consumers.join(", "),
            )
        })
    });
    if layer_hover.is_some() {
        return layer_hover;
    }
    let error_hover = results.unused_tagged_errors.iter().find_map(|finding| {
        let error = &finding.error;
        let anchor = NamedAnchor {
            path: &error.path,
            line: error.line,
            col: error.col,
            name: &error.name,
        };
        anchor_hover(&anchor, file_path, position, mapper, || {
            format!(
                "**fallow**: Tagged error {} is never constructed or failed with in this project.",
                format_inline_code(&error.name),
            )
        })
    });
    if error_hover.is_some() {
        return error_hover;
    }
    results.unproduced_catch_tags.iter().find_map(|finding| {
        let catch_tag = &finding.catch_tag;
        let anchor = NamedAnchor {
            path: &catch_tag.path,
            line: catch_tag.line,
            col: catch_tag.col,
            name: &catch_tag.tag,
        };
        anchor_hover(&anchor, file_path, position, mapper, || {
            format!(
                "**fallow**: No code in this project produces tag {}, so this handler never runs.",
                format_inline_code(&catch_tag.tag),
            )
        })
    })
}

/// Check if the position is on an unused Svelte dispatched event anchor.
fn check_unused_svelte_event(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    results.unused_svelte_events.iter().find_map(|finding| {
        let e = &finding.event;
        let anchor = NamedAnchor {
            path: &e.path,
            line: e.line,
            col: e.col,
            name: &e.event_name,
        };
        anchor_hover(&anchor, file_path, position, mapper, || {
            format!(
                "**fallow**: Event {} is dispatched but listened to nowhere in this project.",
                format_inline_code(&e.event_name),
            )
        })
    })
}

/// Check if the position is on an unused Next.js server action.
fn check_unused_server_action(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    results.unused_server_actions.iter().find_map(|finding| {
        let a = &finding.action;
        let anchor = NamedAnchor {
            path: &a.path,
            line: a.line,
            col: a.col,
            name: &a.action_name,
        };
        anchor_hover(&anchor, file_path, position, mapper, || {
            format!(
                "**fallow**: Server action {} is exported from a \"use server\" file but no code in this project references it.",
                format_inline_code(&a.action_name),
            )
        })
    })
}

/// Check if the position is on an unused SvelteKit `load()` return-object key.
fn check_unused_load_data_key(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    results.unused_load_data_keys.iter().find_map(|finding| {
        let k = &finding.key;
        let anchor = NamedAnchor {
            path: &k.path,
            line: k.line,
            col: k.col,
            name: &k.key_name,
        };
        anchor_hover(&anchor, file_path, position, mapper, || {
            format!(
                "**fallow**: load() return key {} is read by no consumer (sibling +page.svelte data.<key> or project-wide page.data.<key>).",
                format_inline_code(&k.key_name),
            )
        })
    })
}

/// Check if the position is on a React component prop anchor and surface the
/// DESCRIPTIVE per-prop usage hover: whether the prop is read in the component
/// body and how many call sites pass it. Ambient editor context, never a
/// finding. The cursor must fall within the prop-name span (line + `[col, col +
/// name.len())`), matching the React `unused-component-prop` anchor convention.
fn check_react_prop_intel(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    for intel in &results.react_component_intel {
        if intel.path != file_path {
            continue;
        }
        for prop in &intel.props {
            let prop_line = prop.anchor_line.saturating_sub(1);
            if prop_line != position.line {
                continue;
            }
            let (start_col, end_col) =
                mapper.utf16_col_span(&intel.path, prop_line, prop.anchor_col, &prop.name);
            if !position_in_span(position, start_col, end_col) {
                continue;
            }

            let read = if prop.used_in_body {
                "read in body"
            } else {
                "not read in body"
            };
            let sites = if prop.passed_from_sites == 1 {
                "1 call site".to_string()
            } else {
                format!("{} call sites", prop.passed_from_sites)
            };
            let mut value = format!(
                "**fallow**: prop {}: {read} · passed from {sites}",
                format_inline_code(&prop.name),
            );
            // When the prop is the root of a forwarding chain, append the ambient
            // drill trace: `forwarded N levels: A > B > C`. The hop names are user
            // identifiers, so route each through `format_inline_code`.
            if let Some(drill) = &prop.drill {
                let levels = if drill.depth == 1 { "level" } else { "levels" };
                let chain = drill
                    .hops
                    .iter()
                    .map(|h| format_inline_code(h))
                    .collect::<Vec<_>>()
                    .join(" > ");
                let _ = write!(value, "\n\nforwarded {} {levels}: {chain}", drill.depth);
            }

            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value,
                }),
                range: Some(span_range(prop_line, start_col, end_col)),
            });
        }
    }

    None
}

/// Check if the position is on a React component definition anchor and surface
/// the DESCRIPTIVE component summary hover (the same render/prop/hook breakdown
/// as the code lens). Ambient editor context, never a finding. The cursor must
/// fall within the component-name span on the anchor line.
fn check_react_component_intel(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    for intel in &results.react_component_intel {
        if intel.path != file_path {
            continue;
        }
        let component_line = intel.anchor_line.saturating_sub(1);
        if component_line != position.line {
            continue;
        }
        let (start_col, end_col) = mapper.utf16_col_span(
            &intel.path,
            component_line,
            intel.anchor_col,
            &intel.component_name,
        );
        if !position_in_span(position, start_col, end_col) {
            continue;
        }

        let value = format!(
            "**fallow**: component {}: {}",
            format_inline_code(&intel.component_name),
            react_component_summary(intel),
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(span_range(component_line, start_col, end_col)),
        });
    }

    None
}

/// Build the component summary line for the hover, matching the code-lens
/// title format: `rendered 12x (8 parents) · 5 props · 9 hooks (4 state, ...)`.
/// Zero segments are omitted; singular/plural is honored. The hook segment and
/// the pluralization come from `code_lens`, so the lens title and this line
/// cannot describe the same component differently; only the empty-state string
/// is surface-specific.
fn react_component_summary(intel: &fallow_api::editor_results::ReactComponentIntel) -> String {
    let mut segments: Vec<String> = Vec::new();
    if intel.render_sites > 0 {
        let parents = pluralize(intel.distinct_parents, "parent");
        segments.push(format!("rendered {}x ({parents})", intel.render_sites));
    }
    if intel.prop_count > 0 {
        segments.push(pluralize(u32::from(intel.prop_count), "prop"));
    }
    if let Some(hooks) = react_hook_segment(&intel.hooks) {
        segments.push(hooks);
    }
    if segments.is_empty() {
        return "rendered nowhere, no props, no hooks".to_string();
    }
    segments.join(" · ")
}

/// Check if the position is on an unresolved import.
fn check_unresolved_import(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    for import in &results.unresolved_imports {
        if import.import.path != file_path {
            continue;
        }
        let import_line = import.import.line.saturating_sub(1);
        if import_line != position.line {
            continue;
        }
        let start_col = mapper.utf16_col(
            &import.import.path,
            import_line,
            import.import.specifier_col,
        );
        let width =
            u32::try_from(import.import.specifier.encode_utf16().count()).unwrap_or(u32::MAX);
        let end_col = start_col.saturating_add(width).saturating_add(2);
        if !position_in_span(position, start_col, end_col) {
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
            range: Some(span_range(import_line, start_col, end_col)),
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
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    for group in &duplication.clone_groups {
        for instance in &group.instances {
            if instance.file != file_path {
                continue;
            }

            let start_line = (instance.start_line as u32).saturating_sub(1);
            let end_line = (instance.end_line as u32).saturating_sub(1);
            let start_col = mapper.utf16_col(&instance.file, start_line, instance.start_col as u32);
            let end_col = mapper.utf16_col(&instance.file, end_line, instance.end_col as u32);

            if position.line < start_line || position.line > end_line {
                continue;
            }

            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: duplication_hover_markdown(group, instance),
                }),
                range: Some(Range {
                    start: Position {
                        line: start_line,
                        character: start_col,
                    },
                    end: Position {
                        line: end_line,
                        character: end_col,
                    },
                }),
            });
        }
    }

    None
}

/// Build the markdown body for a duplication hover: the block size plus up to
/// ten other instance locations and a "... and N more" overflow line.
fn duplication_hover_markdown(
    group: &fallow_api::editor_duplicates::CloneGroup,
    instance: &fallow_api::editor_duplicates::CloneInstance,
) -> String {
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
        .filter(|other| !(other.file == instance.file && other.start_line == instance.start_line))
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

    value
}

#[cfg(test)]
#[path = "hover_tests.rs"]
mod tests;
