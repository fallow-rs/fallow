use std::fmt::Write;
use std::path::Path;

use ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Range};

use fallow_api::editor_results::SecurityFindingKind;
use fallow_api::{
    EditorAnalysisResults as AnalysisResults, EditorDuplicationReport as DuplicationReport,
};

use crate::diagnostics::security::security_label;
use crate::markdown::format_inline_code;
use crate::position::PositionMapper;

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

fn utf16_span(
    mapper: &mut PositionMapper,
    path: &Path,
    line: u32,
    col: u32,
    text: &str,
) -> (u32, u32) {
    mapper.utf16_col_span(path, line, col, text)
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

fn line_range_from_byte_col(
    mapper: &mut PositionMapper,
    path: &Path,
    line: u32,
    col: u32,
) -> Range {
    let start = mapper.utf16_col(path, line, col);
    Range {
        start: Position {
            line,
            character: start,
        },
        end: Position {
            line,
            character: u32::MAX,
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
            let (start_col, end_col) = utf16_span(
                mapper,
                &export.path,
                export_line,
                export.col,
                &export.export_name,
            );
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
        let (start_col, end_col) = utf16_span(
            mapper,
            &usage.path,
            usage_line,
            usage.col,
            &usage.export_name,
        );
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
    let (start_col, end_col) = utf16_span(
        mapper,
        &member.path,
        member_line,
        member.col,
        &member.member_name,
    );
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

/// Check if the position is on an unrendered Vue/Svelte component anchor.
fn check_unrendered_component(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
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
        let (start_col, end_col) =
            utf16_span(mapper, &c.path, component_line, c.col, &c.component_name);
        if !position_in_span(position, start_col, end_col) {
            continue;
        }

        // Lit: `component_name` is the registered TAG; render it as a custom
        // element to match the CLI human / markdown formatters.
        let value = if c.framework == "lit" {
            format!(
                "**fallow**: Custom element {} is registered but rendered in no template.",
                format_inline_code(&format!("<{}>", c.component_name)),
            )
        } else {
            format!(
                "**fallow**: Component {} is reachable but rendered nowhere in this project.",
                format_inline_code(&c.component_name),
            )
        };

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

/// Check if the position is on an unused component prop anchor.
fn check_unused_component_prop(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
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
        let (start_col, end_col) = utf16_span(mapper, &p.path, prop_line, p.col, &p.prop_name);
        if !position_in_span(position, start_col, end_col) {
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
            range: Some(span_range(prop_line, start_col, end_col)),
        });
    }

    None
}

/// Check if the position is on an unused Vue component emit anchor.
fn check_unused_component_emit(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
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
        let (start_col, end_col) = utf16_span(mapper, &e.path, emit_line, e.col, &e.emit_name);
        if !position_in_span(position, start_col, end_col) {
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
            range: Some(span_range(emit_line, start_col, end_col)),
        });
    }

    None
}

/// Check if the position is on an unused Angular component input anchor.
fn check_unused_component_input(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    for finding in &results.unused_component_inputs {
        let i = &finding.input;
        if i.path != file_path {
            continue;
        }
        let input_line = i.line.saturating_sub(1);
        if input_line != position.line {
            continue;
        }
        let (start_col, end_col) = utf16_span(mapper, &i.path, input_line, i.col, &i.input_name);
        if !position_in_span(position, start_col, end_col) {
            continue;
        }

        let value = format!(
            "**fallow**: Input {} is declared but read nowhere in this component.",
            format_inline_code(&i.input_name),
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(span_range(input_line, start_col, end_col)),
        });
    }

    None
}

/// Check if the position is on an unused Angular component output anchor.
fn check_unused_component_output(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    for finding in &results.unused_component_outputs {
        let o = &finding.output;
        if o.path != file_path {
            continue;
        }
        let output_line = o.line.saturating_sub(1);
        if output_line != position.line {
            continue;
        }
        let (start_col, end_col) = utf16_span(mapper, &o.path, output_line, o.col, &o.output_name);
        if !position_in_span(position, start_col, end_col) {
            continue;
        }

        let value = format!(
            "**fallow**: Output {} is declared but emitted nowhere in this component.",
            format_inline_code(&o.output_name),
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(span_range(output_line, start_col, end_col)),
        });
    }

    None
}

/// Check if the position is on an unused Svelte dispatched event anchor.
fn check_unused_svelte_event(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    for finding in &results.unused_svelte_events {
        let e = &finding.event;
        if e.path != file_path {
            continue;
        }
        let event_line = e.line.saturating_sub(1);
        if event_line != position.line {
            continue;
        }
        let (start_col, end_col) = utf16_span(mapper, &e.path, event_line, e.col, &e.event_name);
        if !position_in_span(position, start_col, end_col) {
            continue;
        }

        let value = format!(
            "**fallow**: Event {} is dispatched but listened to nowhere in this project.",
            format_inline_code(&e.event_name),
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(span_range(event_line, start_col, end_col)),
        });
    }

    None
}

/// Check if the position is on an unused Next.js server action.
fn check_unused_server_action(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
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
        let (start_col, end_col) = utf16_span(mapper, &a.path, action_line, a.col, &a.action_name);
        if !position_in_span(position, start_col, end_col) {
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
            range: Some(span_range(action_line, start_col, end_col)),
        });
    }

    None
}

/// Check if the position is on an unused SvelteKit `load()` return-object key.
fn check_unused_load_data_key(
    results: &AnalysisResults,
    file_path: &Path,
    position: Position,
    mapper: &mut PositionMapper,
) -> Option<Hover> {
    for finding in &results.unused_load_data_keys {
        let k = &finding.key;
        if k.path != file_path {
            continue;
        }
        let key_line = k.line.saturating_sub(1);
        if key_line != position.line {
            continue;
        }
        let (start_col, end_col) = utf16_span(mapper, &k.path, key_line, k.col, &k.key_name);
        if !position_in_span(position, start_col, end_col) {
            continue;
        }

        let value = format!(
            "**fallow**: load() return key {} is read by no consumer (sibling +page.svelte data.<key> or project-wide page.data.<key>).",
            format_inline_code(&k.key_name),
        );

        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(span_range(key_line, start_col, end_col)),
        });
    }

    None
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
                utf16_span(mapper, &intel.path, prop_line, prop.anchor_col, &prop.name);
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
        let (start_col, end_col) = utf16_span(
            mapper,
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
/// Zero segments are omitted; singular/plural is honored.
fn react_component_summary(intel: &fallow_api::editor_results::ReactComponentIntel) -> String {
    let mut segments: Vec<String> = Vec::new();
    if intel.render_sites > 0 {
        let parents = intel_pluralize(intel.distinct_parents, "parent");
        segments.push(format!("rendered {}x ({parents})", intel.render_sites));
    }
    if intel.prop_count > 0 {
        segments.push(intel_pluralize(u32::from(intel.prop_count), "prop"));
    }
    if let Some(hooks) = intel_hook_segment(&intel.hooks) {
        segments.push(hooks);
    }
    if segments.is_empty() {
        return "rendered nowhere, no props, no hooks".to_string();
    }
    segments.join(" · ")
}

/// `N hooks (a state, b effect, ...)` or `None` when the component uses no
/// hooks (kind sub-counts omitted when zero).
fn intel_hook_segment(hooks: &fallow_api::editor_results::ReactHookSummary) -> Option<String> {
    let total = u32::from(hooks.state)
        + u32::from(hooks.effect)
        + u32::from(hooks.memo)
        + u32::from(hooks.callback)
        + u32::from(hooks.custom);
    if total == 0 {
        return None;
    }
    let mut breakdown: Vec<String> = Vec::new();
    for (count, label) in [
        (hooks.state, "state"),
        (hooks.effect, "effect"),
        (hooks.memo, "memo"),
        (hooks.callback, "callback"),
        (hooks.custom, "custom"),
    ] {
        if count > 0 {
            breakdown.push(format!("{count} {label}"));
        }
    }
    let head = intel_pluralize(total, "hook");
    if breakdown.is_empty() {
        Some(head)
    } else {
        Some(format!("{head} ({})", breakdown.join(", ")))
    }
}

/// `count + " " + noun`, appending `s` when count is not 1.
fn intel_pluralize(count: u32, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
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
mod tests {
    use super::*;
    use std::path::PathBuf;

    use fallow_api::editor_duplicates::{CloneGroup, CloneInstance, DuplicationStats};
    use fallow_api::editor_extract::MemberKind;
    use fallow_api::editor_results::{
        ExportUsage, ReactComponentIntel, ReactHookSummary, ReactPropDrill, ReactPropIntel,
        ReferenceLocation, SecuritySeverity, UnresolvedImport, UnresolvedImportFinding,
        UnusedClassMemberFinding, UnusedEnumMemberFinding, UnusedExport, UnusedExportFinding,
        UnusedFile, UnusedFileFinding, UnusedMember, UnusedStoreMemberFinding, UnusedTypeFinding,
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos);
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("helper"));
        assert!(value.contains("not imported"));
        let range = hover.range.unwrap();
        assert_eq!(range.start.line, 4);
        assert_eq!(range.start.character, 7);
        assert_eq!(range.end.character, 7 + "helper".len() as u32);
    }

    #[test]
    fn hover_on_unused_export_uses_utf16_columns() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let path = root.join("src/non_ascii.ts");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let source = "const emoji = \"🎉\"; export const helper = 1;\n";
        std::fs::write(&path, source).expect("write fixture");
        let byte_col = source.find("helper").expect("helper") as u32;
        let utf16_col = source[..byte_col as usize].encode_utf16().count() as u32;
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "helper".to_string(),
                is_type_only: false,
                line: 1,
                col: byte_col,
                span_start: byte_col,
                is_re_export: false,
            }));
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 0,
            character: utf16_col,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let range = hover.range.expect("range");
        assert_eq!(range.start.character, utf16_col);
        assert_eq!(
            range.end.character,
            utf16_col + "helper".encode_utf16().count() as u32
        );
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos);
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path_a, pos).unwrap();
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
        let hover = build_hover_for_test(&results, &duplication, &path, pos);
        assert!(hover.is_none());

        let pos = Position {
            line: 20,
            character: 0,
        };
        let hover = build_hover_for_test(&results, &duplication, &path, pos);
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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
        let hover = build_hover_for_test(&results, &duplication, &path, pos);
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
        let hover = build_hover_for_test(&results, &duplication, &path, pos);
        assert!(hover.is_none());

        let pos = Position {
            line: 4,
            character: 3,
        };
        let hover = build_hover_for_test(&results, &duplication, &path, pos);
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
        let hover = build_hover_for_test(&results, &duplication, &path_a, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_some());

        let pos = Position {
            line: 0,
            character: 16,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_some());

        let pos = Position {
            line: 0,
            character: 17,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());

        let pos = Position {
            line: 0,
            character: 9,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
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
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_some());

        let pos = Position {
            line: 0,
            character: 9,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_some());

        let pos = Position {
            line: 0,
            character: 10,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
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
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_some());

        let pos = Position {
            line: 2,
            character: 7,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
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
        let hover = build_hover_for_test(&results, &duplication, &path_main, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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
        assert!(build_hover_for_test(&results, &duplication, &path_b, pos).is_none());
    }

    fn tainted_sink_finding(path: PathBuf) -> fallow_api::editor_results::SecurityFinding {
        fallow_api::editor_results::SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_api::editor_results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: fallow_api::editor_results::SecurityFindingKind::TaintedSink,
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
            reachability: Some(fallow_api::editor_results::SecurityReachability {
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
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
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());

        // Before the anchor column.
        let pos = Position {
            line: 7,
            character: 2,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
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

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("`[click](command:vscode.open?evil)`"));
    }

    // -------------------------------------------------------------------------
    // Unrendered component (lines 466-503)
    // -------------------------------------------------------------------------

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test string lengths are trivially small"
    )]
    fn hover_on_unrendered_component() {
        let root = test_root();
        let path = root.join("src/components/MyCard.vue");
        let mut results = AnalysisResults::default();
        results.unrendered_components.push(
            fallow_api::editor_results::UnrenderedComponentFinding::with_actions(
                fallow_api::editor_results::UnrenderedComponent {
                    path: path.clone(),
                    component_name: "MyCard".to_string(),
                    framework: "vue".to_string(),
                    reachable_via: None,
                    line: 1,
                    col: 0,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 0,
            character: 3,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("MyCard"));
        assert!(value.contains("rendered nowhere"));
        let range = hover.range.unwrap();
        assert_eq!(range.start.line, 0);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.character, "MyCard".len() as u32);
    }

    #[test]
    fn hover_on_unrendered_component_wrong_line_returns_none() {
        let root = test_root();
        let path = root.join("src/components/MyCard.vue");
        let mut results = AnalysisResults::default();
        results.unrendered_components.push(
            fallow_api::editor_results::UnrenderedComponentFinding::with_actions(
                fallow_api::editor_results::UnrenderedComponent {
                    path: path.clone(),
                    component_name: "MyCard".to_string(),
                    framework: "vue".to_string(),
                    reachable_via: None,
                    line: 1,
                    col: 0,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 5,
            character: 0,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
    }

    #[test]
    fn hover_on_unrendered_component_wrong_col_returns_none() {
        let root = test_root();
        let path = root.join("src/components/MyCard.vue");
        let mut results = AnalysisResults::default();
        results.unrendered_components.push(
            fallow_api::editor_results::UnrenderedComponentFinding::with_actions(
                fallow_api::editor_results::UnrenderedComponent {
                    path: path.clone(),
                    component_name: "MyCard".to_string(),
                    framework: "vue".to_string(),
                    reachable_via: None,
                    line: 1,
                    col: 5,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 0,
            character: 2,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
    }

    // -------------------------------------------------------------------------
    // Unused component prop (lines 516-553)
    // -------------------------------------------------------------------------

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test string lengths are trivially small"
    )]
    fn hover_on_unused_component_prop() {
        let root = test_root();
        let path = root.join("src/components/Button.vue");
        let mut results = AnalysisResults::default();
        results.unused_component_props.push(
            fallow_api::editor_results::UnusedComponentPropFinding::with_actions(
                fallow_api::editor_results::UnusedComponentProp {
                    path: path.clone(),
                    component_name: "Button".to_string(),
                    prop_name: "variant".to_string(),
                    line: 3,
                    col: 2,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 2,
            character: 5,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("variant"));
        assert!(value.contains("declared but referenced nowhere"));
        let range = hover.range.unwrap();
        assert_eq!(range.start.line, 2);
        assert_eq!(range.start.character, 2);
        assert_eq!(range.end.character, 2 + "variant".len() as u32);
    }

    #[test]
    fn hover_on_unused_component_prop_wrong_col_returns_none() {
        let root = test_root();
        let path = root.join("src/components/Button.vue");
        let mut results = AnalysisResults::default();
        results.unused_component_props.push(
            fallow_api::editor_results::UnusedComponentPropFinding::with_actions(
                fallow_api::editor_results::UnusedComponentProp {
                    path: path.clone(),
                    component_name: "Button".to_string(),
                    prop_name: "variant".to_string(),
                    line: 3,
                    col: 2,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 2,
            character: 50,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
    }

    // -------------------------------------------------------------------------
    // Unused component emit (lines 566-603)
    // -------------------------------------------------------------------------

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test string lengths are trivially small"
    )]
    fn hover_on_unused_component_emit() {
        let root = test_root();
        let path = root.join("src/components/Form.vue");
        let mut results = AnalysisResults::default();
        results.unused_component_emits.push(
            fallow_api::editor_results::UnusedComponentEmitFinding::with_actions(
                fallow_api::editor_results::UnusedComponentEmit {
                    path: path.clone(),
                    component_name: "Form".to_string(),
                    emit_name: "submit".to_string(),
                    line: 5,
                    col: 4,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 4,
            character: 7,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("submit"));
        assert!(value.contains("declared but emitted nowhere"));
        let range = hover.range.unwrap();
        assert_eq!(range.start.line, 4);
        assert_eq!(range.start.character, 4);
        assert_eq!(range.end.character, 4 + "submit".len() as u32);
    }

    #[test]
    fn hover_on_unused_component_emit_wrong_col_returns_none() {
        let root = test_root();
        let path = root.join("src/components/Form.vue");
        let mut results = AnalysisResults::default();
        results.unused_component_emits.push(
            fallow_api::editor_results::UnusedComponentEmitFinding::with_actions(
                fallow_api::editor_results::UnusedComponentEmit {
                    path: path.clone(),
                    component_name: "Form".to_string(),
                    emit_name: "submit".to_string(),
                    line: 5,
                    col: 4,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 4,
            character: 100,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
    }

    // -------------------------------------------------------------------------
    // Unused Angular component input (lines 616-653)
    // -------------------------------------------------------------------------

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test string lengths are trivially small"
    )]
    fn hover_on_unused_component_input() {
        let root = test_root();
        let path = root.join("src/app/card/card.component.ts");
        let mut results = AnalysisResults::default();
        results.unused_component_inputs.push(
            fallow_api::editor_results::UnusedComponentInputFinding::with_actions(
                fallow_api::editor_results::UnusedComponentInput {
                    path: path.clone(),
                    component_name: "CardComponent".to_string(),
                    input_name: "title".to_string(),
                    line: 7,
                    col: 2,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 6,
            character: 4,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("title"));
        assert!(value.contains("declared but read nowhere"));
        let range = hover.range.unwrap();
        assert_eq!(range.start.line, 6);
        assert_eq!(range.start.character, 2);
        assert_eq!(range.end.character, 2 + "title".len() as u32);
    }

    #[test]
    fn hover_on_unused_component_input_wrong_line_returns_none() {
        let root = test_root();
        let path = root.join("src/app/card/card.component.ts");
        let mut results = AnalysisResults::default();
        results.unused_component_inputs.push(
            fallow_api::editor_results::UnusedComponentInputFinding::with_actions(
                fallow_api::editor_results::UnusedComponentInput {
                    path: path.clone(),
                    component_name: "CardComponent".to_string(),
                    input_name: "title".to_string(),
                    line: 7,
                    col: 2,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 20,
            character: 2,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
    }

    // -------------------------------------------------------------------------
    // Unused Angular component output (lines 666-703)
    // -------------------------------------------------------------------------

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test string lengths are trivially small"
    )]
    fn hover_on_unused_component_output() {
        let root = test_root();
        let path = root.join("src/app/counter/counter.component.ts");
        let mut results = AnalysisResults::default();
        results.unused_component_outputs.push(
            fallow_api::editor_results::UnusedComponentOutputFinding::with_actions(
                fallow_api::editor_results::UnusedComponentOutput {
                    path: path.clone(),
                    component_name: "CounterComponent".to_string(),
                    output_name: "changed".to_string(),
                    line: 10,
                    col: 4,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 9,
            character: 6,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("changed"));
        assert!(value.contains("declared but emitted nowhere"));
        let range = hover.range.unwrap();
        assert_eq!(range.start.line, 9);
        assert_eq!(range.start.character, 4);
        assert_eq!(range.end.character, 4 + "changed".len() as u32);
    }

    #[test]
    fn hover_on_unused_component_output_wrong_col_returns_none() {
        let root = test_root();
        let path = root.join("src/app/counter/counter.component.ts");
        let mut results = AnalysisResults::default();
        results.unused_component_outputs.push(
            fallow_api::editor_results::UnusedComponentOutputFinding::with_actions(
                fallow_api::editor_results::UnusedComponentOutput {
                    path: path.clone(),
                    component_name: "CounterComponent".to_string(),
                    output_name: "changed".to_string(),
                    line: 10,
                    col: 4,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 9,
            character: 200,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
    }

    // -------------------------------------------------------------------------
    // Unused Svelte dispatched event (lines 716-753)
    // -------------------------------------------------------------------------

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test string lengths are trivially small"
    )]
    fn hover_on_unused_svelte_event() {
        let root = test_root();
        let path = root.join("src/lib/Notification.svelte");
        let mut results = AnalysisResults::default();
        results.unused_svelte_events.push(
            fallow_api::editor_results::UnusedSvelteEventFinding::with_actions(
                fallow_api::editor_results::UnusedSvelteEvent {
                    path: path.clone(),
                    component_name: "Notification".to_string(),
                    event_name: "close".to_string(),
                    line: 4,
                    col: 0,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 3,
            character: 3,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("close"));
        assert!(value.contains("dispatched but listened to nowhere"));
        let range = hover.range.unwrap();
        assert_eq!(range.start.line, 3);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.character, "close".len() as u32);
    }

    #[test]
    fn hover_on_unused_svelte_event_wrong_line_returns_none() {
        let root = test_root();
        let path = root.join("src/lib/Notification.svelte");
        let mut results = AnalysisResults::default();
        results.unused_svelte_events.push(
            fallow_api::editor_results::UnusedSvelteEventFinding::with_actions(
                fallow_api::editor_results::UnusedSvelteEvent {
                    path: path.clone(),
                    component_name: "Notification".to_string(),
                    event_name: "close".to_string(),
                    line: 4,
                    col: 0,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 10,
            character: 0,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
    }

    // -------------------------------------------------------------------------
    // Unused Next.js server action (lines 766-803)
    // -------------------------------------------------------------------------

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test string lengths are trivially small"
    )]
    fn hover_on_unused_server_action() {
        let root = test_root();
        let path = root.join("src/app/actions.ts");
        let mut results = AnalysisResults::default();
        results.unused_server_actions.push(
            fallow_api::editor_results::UnusedServerActionFinding::with_actions(
                fallow_api::editor_results::UnusedServerAction {
                    path: path.clone(),
                    action_name: "deleteUser".to_string(),
                    line: 8,
                    col: 16,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 7,
            character: 20,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("deleteUser"));
        assert!(value.contains("use server"));
        assert!(value.contains("no code in this project references it"));
        let range = hover.range.unwrap();
        assert_eq!(range.start.line, 7);
        assert_eq!(range.start.character, 16);
        assert_eq!(range.end.character, 16 + "deleteUser".len() as u32);
    }

    #[test]
    fn hover_on_unused_server_action_wrong_col_returns_none() {
        let root = test_root();
        let path = root.join("src/app/actions.ts");
        let mut results = AnalysisResults::default();
        results.unused_server_actions.push(
            fallow_api::editor_results::UnusedServerActionFinding::with_actions(
                fallow_api::editor_results::UnusedServerAction {
                    path: path.clone(),
                    action_name: "deleteUser".to_string(),
                    line: 8,
                    col: 16,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 7,
            character: 0,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
    }

    // -------------------------------------------------------------------------
    // Unused SvelteKit load() return key (lines 816-853)
    // -------------------------------------------------------------------------

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test string lengths are trivially small"
    )]
    fn hover_on_unused_load_data_key() {
        let root = test_root();
        let path = root.join("src/routes/blog/+page.server.ts");
        let mut results = AnalysisResults::default();
        results.unused_load_data_keys.push(
            fallow_api::editor_results::UnusedLoadDataKeyFinding::with_actions(
                fallow_api::editor_results::UnusedLoadDataKey {
                    path: path.clone(),
                    key_name: "posts".to_string(),
                    line: 12,
                    col: 4,
                    route_dir: Some("src/routes/blog".to_string()),
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 11,
            character: 6,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("posts"));
        assert!(value.contains("load()"));
        assert!(value.contains("read by no consumer"));
        let range = hover.range.unwrap();
        assert_eq!(range.start.line, 11);
        assert_eq!(range.start.character, 4);
        assert_eq!(range.end.character, 4 + "posts".len() as u32);
    }

    #[test]
    fn hover_on_unused_load_data_key_wrong_line_returns_none() {
        let root = test_root();
        let path = root.join("src/routes/blog/+page.server.ts");
        let mut results = AnalysisResults::default();
        results.unused_load_data_keys.push(
            fallow_api::editor_results::UnusedLoadDataKeyFinding::with_actions(
                fallow_api::editor_results::UnusedLoadDataKey {
                    path: path.clone(),
                    key_name: "posts".to_string(),
                    line: 12,
                    col: 4,
                    route_dir: None,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 0,
            character: 4,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
    }

    #[test]
    fn hover_on_unused_load_data_key_wrong_col_returns_none() {
        let root = test_root();
        let path = root.join("src/routes/blog/+page.server.ts");
        let mut results = AnalysisResults::default();
        results.unused_load_data_keys.push(
            fallow_api::editor_results::UnusedLoadDataKeyFinding::with_actions(
                fallow_api::editor_results::UnusedLoadDataKey {
                    path: path.clone(),
                    key_name: "posts".to_string(),
                    line: 12,
                    col: 4,
                    route_dir: None,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 11,
            character: 0,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
    }

    // -------------------------------------------------------------------------
    // Security: ClientServerLeak next step (line 198-200), dead-code branch (202-205)
    // -------------------------------------------------------------------------

    #[test]
    fn hover_on_client_server_leak_security_candidate() {
        let root = test_root();
        let path = root.join("src/client/Secrets.tsx");
        let finding = fallow_api::editor_results::SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_api::editor_results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: fallow_api::editor_results::SecurityFindingKind::ClientServerLeak,
            category: None,
            cwe: None,
            path: path.clone(),
            line: 3,
            col: 0,
            evidence: "process.env.SECRET_KEY imported into client bundle".to_string(),
            source_backed: false,
            source_read: None,
            severity: fallow_api::editor_results::SecuritySeverity::High,
            trace: vec![],
            actions: vec![],
            dead_code: None,
            reachability: None,
            runtime: None,
        };
        let mut results = AnalysisResults::default();
        results.security_findings.push(finding);
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 2,
            character: 5,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("client-server-leak"));
        assert!(value.contains("source-backed no"));
        assert!(value.contains("reachable from a runtime entry point unknown"));
        assert!(value.contains("type-only"));
        assert!(value.contains("process.env.SECRET_KEY"));
    }

    #[test]
    fn hover_on_tainted_sink_with_dead_code_context() {
        let root = test_root();
        let path = root.join("src/utils/xss.ts");
        let mut finding = tainted_sink_finding(path.clone());
        finding.dead_code = Some(fallow_api::editor_results::SecurityDeadCodeContext {
            kind: fallow_api::editor_results::SecurityDeadCodeKind::UnusedExport,
            export_name: Some("renderHtml".to_string()),
            line: Some(8),
            guidance: "Verify the dead-code finding and delete the code if safe before hardening."
                .to_string(),
        });
        let mut results = AnalysisResults::default();
        results.security_findings.push(finding);
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 7,
            character: 10,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("dead-code:"));
        assert!(value.contains("Verify the dead-code finding"));
        assert!(value.contains("Next: verify the dead-code finding"));
    }

    #[test]
    fn hover_on_tainted_sink_with_boundary_crossing() {
        let root = test_root();
        let path = root.join("src/api/query.ts");
        let mut finding = tainted_sink_finding(path.clone());
        if let Some(ref mut reach) = finding.reachability {
            reach.crosses_boundary = true;
            reach.blast_radius = 7;
        }
        let mut results = AnalysisResults::default();
        results.security_findings.push(finding);
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 7,
            character: 10,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("blast radius 7"));
        assert!(value.contains("crosses an architecture boundary"));
    }

    // -------------------------------------------------------------------------
    // Security: col-before-anchor guard (line 107-108)
    // -------------------------------------------------------------------------

    #[test]
    fn hover_on_security_candidate_col_before_anchor_returns_none() {
        let root = test_root();
        let path = root.join("src/render.ts");
        let mut results = AnalysisResults::default();
        results
            .security_findings
            .push(tainted_sink_finding(path.clone()));
        let duplication = DuplicationReport::default();

        let pos = Position {
            line: 7,
            character: 5,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
    }

    // -------------------------------------------------------------------------
    // build_hover dispatch lines 43-73 (new check_* returns exercised via
    // priority ordering: unrendered wins over prop, etc.)
    // -------------------------------------------------------------------------

    #[test]
    fn unused_file_takes_priority_over_unrendered_component() {
        let root = test_root();
        let path = root.join("src/components/Dead.vue");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: path.clone(),
            }));
        results.unrendered_components.push(
            fallow_api::editor_results::UnrenderedComponentFinding::with_actions(
                fallow_api::editor_results::UnrenderedComponent {
                    path: path.clone(),
                    component_name: "Dead".to_string(),
                    framework: "vue".to_string(),
                    reachable_via: None,
                    line: 1,
                    col: 0,
                },
            ),
        );
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 0,
            character: 0,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(
            value.contains("not imported"),
            "Expected unused-file hover, got: {value}"
        );
    }

    // -------------------------------------------------------------------------
    // Duplication: branch where others is non-empty (line 919-920 + 979-983)
    // -------------------------------------------------------------------------

    #[test]
    fn hover_duplication_single_other_shows_singular_word() {
        let root = test_root();
        let path_a = root.join("src/alpha.ts");
        let path_b = root.join("src/beta.ts");
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
                        fragment: "dup".to_string(),
                    },
                    CloneInstance {
                        file: path_b,
                        start_line: 10,
                        end_line: 14,
                        start_col: 0,
                        end_col: 10,
                        fragment: "dup".to_string(),
                    },
                ],
                token_count: 20,
                line_count: 5,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 2,
                files_with_clones: 2,
                total_lines: 50,
                duplicated_lines: 10,
                total_tokens: 200,
                duplicated_tokens: 40,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 20.0,
                clone_groups_below_min_occurrences: 0,
            },
        };

        let pos = Position {
            line: 2,
            character: 0,
        };
        let hover = build_hover_for_test(&results, &duplication, &path_a, pos).unwrap();
        let value = markup_value(&hover);
        assert!(
            value.contains("1 other instance"),
            "Expected singular form, got: {value}"
        );
        assert!(value.contains("beta.ts"));
    }

    // -------------------------------------------------------------------------
    // Duplication: more-than-10-others overflow (lines 976-983)
    // -------------------------------------------------------------------------

    #[test]
    fn hover_duplication_more_than_10_others_shows_overflow_line() {
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
        for i in 1..=12_usize {
            instances.push(CloneInstance {
                file: root.join(format!("src/other{i}.ts")),
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
            stats: DuplicationStats {
                total_files: 13,
                files_with_clones: 13,
                total_lines: 200,
                duplicated_lines: 65,
                total_tokens: 500,
                duplicated_tokens: 390,
                clone_groups: 1,
                clone_instances: 13,
                duplication_percentage: 32.0,
                clone_groups_below_min_occurrences: 0,
            },
        };

        let pos = Position {
            line: 2,
            character: 0,
        };
        let hover = build_hover_for_test(&results, &duplication, &path_main, pos).unwrap();
        let value = markup_value(&hover);
        assert!(
            value.contains("... and 2 more"),
            "Expected overflow line, got: {value}",
        );
    }

    // -------------------------------------------------------------------------
    // Store member hover (lines 417-422 - the store_iter arm)
    // -------------------------------------------------------------------------

    #[test]
    fn hover_on_unused_store_member_wrong_col_returns_none() {
        let root = test_root();
        let path = root.join("src/store.ts");
        let mut results = AnalysisResults::default();
        results
            .unused_store_members
            .push(UnusedStoreMemberFinding::with_actions(UnusedMember {
                path: path.clone(),
                parent_name: "useUserStore".to_string(),
                member_name: "logout".to_string(),
                kind: MemberKind::StoreMember,
                line: 15,
                col: 4,
            }));
        let duplication = DuplicationReport::default();

        let pos = Position {
            line: 14,
            character: 100,
        };
        assert!(build_hover_for_test(&results, &duplication, &path, pos).is_none());
    }

    // -------------------------------------------------------------------------
    // Unresolved import: path-mismatch guard (line 868-873 area)
    // -------------------------------------------------------------------------

    #[test]
    fn hover_on_unresolved_import_wrong_file_returns_none() {
        let root = test_root();
        let path_a = root.join("src/a.ts");
        let path_b = root.join("src/b.ts");
        let mut results = AnalysisResults::default();
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: path_a,
                specifier: "./missing".to_string(),
                line: 1,
                col: 0,
                specifier_col: 10,
            }));
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 0,
            character: 12,
        };
        assert!(build_hover_for_test(&results, &duplication, &path_b, pos).is_none());
    }

    fn card_intel(path: PathBuf) -> ReactComponentIntel {
        ReactComponentIntel {
            path,
            component_name: "Card".to_string(),
            anchor_line: 7,
            anchor_col: 13,
            render_sites: 12,
            distinct_parents: 8,
            prop_count: 2,
            hooks: ReactHookSummary {
                state: 1,
                effect: 1,
                ..ReactHookSummary::default()
            },
            props: vec![
                ReactPropIntel {
                    name: "title".to_string(),
                    anchor_line: 8,
                    anchor_col: 2,
                    used_in_body: true,
                    passed_from_sites: 3,
                    drill: None,
                },
                ReactPropIntel {
                    name: "subtitle".to_string(),
                    anchor_line: 9,
                    anchor_col: 2,
                    used_in_body: false,
                    passed_from_sites: 0,
                    drill: None,
                },
            ],
        }
    }

    #[test]
    fn hover_on_react_prop_read_and_passed() {
        let root = test_root();
        let path = root.join("src/Card.tsx");
        let mut results = AnalysisResults::default();
        results.react_component_intel.push(card_intel(path.clone()));
        let duplication = DuplicationReport::default();
        // On the `title` prop anchor (1-based line 8 -> 0-based 7, col 2-7).
        let pos = Position {
            line: 7,
            character: 3,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("title"), "names the prop: {value}");
        assert!(value.contains("read in body"), "read state: {value}");
        assert!(
            value.contains("passed from 3 call sites"),
            "pass count: {value}"
        );
    }

    #[test]
    fn hover_on_react_prop_unread_and_zero_passes() {
        let root = test_root();
        let path = root.join("src/Card.tsx");
        let mut results = AnalysisResults::default();
        results.react_component_intel.push(card_intel(path.clone()));
        let duplication = DuplicationReport::default();
        // On the `subtitle` prop anchor (1-based line 9 -> 0-based 8).
        let pos = Position {
            line: 8,
            character: 4,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("subtitle"), "names the prop: {value}");
        assert!(value.contains("not read in body"), "read state: {value}");
        // Singular "0 call sites".
        assert!(
            value.contains("passed from 0 call sites"),
            "pass count: {value}"
        );
    }

    #[test]
    fn hover_on_react_prop_single_call_site_is_singular() {
        let root = test_root();
        let path = root.join("src/Card.tsx");
        let mut results = AnalysisResults::default();
        let mut intel = card_intel(path.clone());
        intel.props[0].passed_from_sites = 1;
        results.react_component_intel.push(intel);
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 7,
            character: 3,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(
            value.contains("passed from 1 call site"),
            "singular call site: {value}"
        );
        assert!(!value.contains("1 call sites"), "no plural-s: {value}");
    }

    #[test]
    fn hover_on_react_prop_with_drill_renders_trace() {
        let root = test_root();
        let path = root.join("src/Card.tsx");
        let mut results = AnalysisResults::default();
        let mut intel = card_intel(path.clone());
        // Attach a drill trace to the `title` prop: forwarded 3 levels through
        // Page > Layout > Sidebar > Profile.
        intel.props[0].drill = Some(ReactPropDrill {
            depth: 3,
            hops: vec![
                "Page".to_string(),
                "Layout".to_string(),
                "Sidebar".to_string(),
                "Profile".to_string(),
            ],
        });
        results.react_component_intel.push(intel);
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 7,
            character: 3,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        // The base read/passed line is preserved.
        assert!(value.contains("read in body"), "base read state: {value}");
        assert!(
            value.contains("passed from 3 call sites"),
            "base pass count: {value}"
        );
        // The drill trace renders the depth and the ordered chain.
        assert!(
            value.contains("forwarded 3 levels"),
            "drill depth line: {value}"
        );
        assert!(value.contains("Page"), "chain head: {value}");
        assert!(value.contains("Profile"), "chain tail: {value}");
        assert!(value.contains(" > "), "chain separator: {value}");
    }

    #[test]
    fn hover_on_react_prop_single_level_drill_is_singular() {
        let root = test_root();
        let path = root.join("src/Card.tsx");
        let mut results = AnalysisResults::default();
        let mut intel = card_intel(path.clone());
        intel.props[0].drill = Some(ReactPropDrill {
            depth: 1,
            hops: vec!["Page".to_string()],
        });
        results.react_component_intel.push(intel);
        let duplication = DuplicationReport::default();
        let pos = Position {
            line: 7,
            character: 3,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(
            value.contains("forwarded 1 level:"),
            "singular level: {value}"
        );
        assert!(!value.contains("1 levels"), "no plural-s: {value}");
    }

    #[test]
    fn hover_on_react_component_anchor_shows_summary() {
        let root = test_root();
        let path = root.join("src/Card.tsx");
        let mut results = AnalysisResults::default();
        results.react_component_intel.push(card_intel(path.clone()));
        let duplication = DuplicationReport::default();
        // On the component name anchor (1-based line 7 -> 0-based 6, col 13).
        let pos = Position {
            line: 6,
            character: 15,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        assert!(value.contains("Card"), "names the component: {value}");
        assert!(
            value.contains("rendered 12x (8 parents)"),
            "render: {value}"
        );
        assert!(value.contains("2 props"), "prop count: {value}");
        assert!(
            value.contains("2 hooks (1 state, 1 effect)"),
            "hooks: {value}"
        );
    }

    #[test]
    fn hover_react_prop_finding_wins_over_intel() {
        // A real unused-component-prop finding and the descriptive intel anchor
        // the same prop position; the finding hover must win (it is checked
        // first in build_hover).
        let root = test_root();
        let path = root.join("src/Card.tsx");
        let mut results = AnalysisResults::default();
        results.unused_component_props.push(
            fallow_api::editor_results::UnusedComponentPropFinding::with_actions(
                fallow_api::editor_results::UnusedComponentProp {
                    path: path.clone(),
                    component_name: "Card".to_string(),
                    prop_name: "subtitle".to_string(),
                    line: 9,
                    col: 2,
                },
            ),
        );
        results.react_component_intel.push(card_intel(path.clone()));
        let duplication = DuplicationReport::default();
        // On the `subtitle` anchor (1-based line 9 -> 0-based 8, col 2).
        let pos = Position {
            line: 8,
            character: 3,
        };

        let hover = build_hover_for_test(&results, &duplication, &path, pos).unwrap();
        let value = markup_value(&hover);
        // The finding's wording, not the intel's "not read in body / passed".
        assert!(
            value.contains("referenced nowhere in this component"),
            "finding hover wins: {value}"
        );
        assert!(
            !value.contains("passed from"),
            "intel hover did not fire: {value}"
        );
    }
}
