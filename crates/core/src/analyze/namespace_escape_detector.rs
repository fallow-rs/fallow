//! Report the namespace bindings that hide exports from the unused-export
//! check (issue #5054).

use fallow_config::Severity;
use fallow_types::output_dead_code::NamespaceEscapeFinding;
use fallow_types::results::{
    NamespaceEscape as NamespaceEscapeResult, NamespaceEscapeReason as NamespaceEscapeResultReason,
};

use crate::graph::NamespaceEscapeReason;
use crate::suppress::IssueKind;

use super::unused_exports::{UnusedExportInput, find_namespace_escape_candidates};
use super::{DeadCodeDetectorInput, byte_offset_to_line_col};

/// Find the namespace escapes that hide at least one export, without the
/// suppressed ones, sorted by path, line and column.
pub(super) fn run_namespace_escape_detector(
    input: DeadCodeDetectorInput<'_>,
) -> Vec<NamespaceEscapeFinding> {
    if input.config.rules.namespace_escapes == Severity::Off {
        return Vec::new();
    }
    let graph = input.graph;
    let candidates = find_namespace_escape_candidates(UnusedExportInput {
        graph,
        modules: input.modules,
        config: input.config,
        plugin_result: input.plugin_result,
        suppressions: input.suppressions,
        line_offsets_by_file: input.line_offsets_by_file,
    });
    let mut findings: Vec<NamespaceEscapeFinding> = candidates
        .into_iter()
        .filter_map(|candidate| {
            let (line, col) = byte_offset_to_line_col(
                input.line_offsets_by_file,
                candidate.consumer,
                candidate.import_start,
            );
            if input.suppressions.is_suppressed(
                candidate.consumer,
                line,
                IssueKind::NamespaceEscape,
            ) {
                return None;
            }
            let path = graph
                .modules()
                .get(candidate.consumer.0 as usize)?
                .path
                .clone();
            let module = graph.module(candidate.target)?.path.clone();
            Some(NamespaceEscapeFinding::with_actions(
                NamespaceEscapeResult {
                    path,
                    line,
                    col,
                    local_name: candidate.local_name,
                    module,
                    hidden_exports: candidate.hidden_exports,
                    reason: public_reason(candidate.reason),
                },
            ))
        })
        .collect();
    findings.sort_by(|a, b| {
        (&a.escape.path, a.escape.line, a.escape.col).cmp(&(
            &b.escape.path,
            b.escape.line,
            b.escape.col,
        ))
    });
    findings
}

const fn public_reason(reason: NamespaceEscapeReason) -> NamespaceEscapeResultReason {
    match reason {
        NamespaceEscapeReason::WholeObjectUse => NamespaceEscapeResultReason::WholeObjectUse,
        NamespaceEscapeReason::ReExportedBinding => NamespaceEscapeResultReason::ReExportedBinding,
        NamespaceEscapeReason::NoMemberRead => NamespaceEscapeResultReason::NoMemberRead,
    }
}
