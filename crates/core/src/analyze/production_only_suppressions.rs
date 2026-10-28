//! Keep a suppression that only production mode needs out of the stale list.
//!
//! Production mode excludes test, story, and fixture files from discovery. An
//! export that only those files import is unused under `--production`, so its
//! `unused-export` or `unused-type` suppression is needed there. A run without
//! `--production` credits the test import and does not consume the
//! suppression. Reporting it stale there would leave no comment that is clean
//! in both modes (issue #3825).
//!
//! The same applies to an `unused-enum-member` suppression on a member that
//! only those files read (issue #3962).

use std::cell::OnceCell;

use fallow_config::{ResolvedConfig, Severity};
use fallow_types::extract::ExportName;
use fallow_types::suppress::SuppressionTarget;
use rustc_hash::FxHashSet;

use crate::discover::FileId;
use crate::graph::{ExportSymbol, ModuleGraph, ModuleNode};
use crate::results::{StaleSuppression, UnusedMember};
use crate::suppress::{IssueKind, Suppression, SuppressionContext};

use super::members::{UnusedMemberScanInput, find_unused_members_with_public_api_entry_points};
use super::unused_deps::production_exclude_globset;
use super::{LineOffsetsMap, PostDetectionInput, byte_offset_to_line_col};

/// Collect the stale suppressions of this run, without a suppression that
/// only production mode needs.
pub(super) fn find_stale(input: &PostDetectionInput<'_, '_>) -> Vec<StaleSuppression> {
    let (graph, config) = (input.graph, input.config);
    let production_enum_members: OnceCell<Vec<UnusedMember>> = OnceCell::new();
    input
        .suppressions
        .find_stale_except(graph, config, |file_id, suppression| {
            is_needed_in_production(
                graph,
                config,
                input.line_offsets_by_file,
                file_id,
                suppression,
            ) || is_enum_member_needed_in_production(
                input,
                file_id,
                suppression,
                &production_enum_members,
            )
        })
}

/// Whether production mode would report an enum member that `suppression`
/// covers.
///
/// Production mode excludes a test file, so it does not credit a member
/// access in that file. The check scans the members again without the
/// accesses of production-excluded files. The scan runs once, and only when
/// an unconsumed suppression can cover an enum member.
fn is_enum_member_needed_in_production(
    input: &PostDetectionInput<'_, '_>,
    file_id: FileId,
    suppression: &Suppression,
    production_enum_members: &OnceCell<Vec<UnusedMember>>,
) -> bool {
    let (graph, config) = (input.graph, input.config);
    if config.production
        || config.rules.unused_enum_members == Severity::Off
        || !matches!(
            suppression.target,
            None | Some(SuppressionTarget::Issue(IssueKind::UnusedEnumMember))
        )
    {
        return false;
    }
    let Some(module) = graph.modules.get(file_id.0 as usize) else {
        return false;
    };
    if is_production_excluded(config, module) {
        return false;
    }
    production_enum_members
        .get_or_init(|| find_production_unused_enum_members(input))
        .iter()
        .any(|member| {
            member.path == module.path
                && suppression.matches_issue_kind(member.line, IssueKind::UnusedEnumMember)
                && module.exports.iter().any(|export| {
                    export.name.to_string() == member.parent_name
                        && has_production_reference(graph, config, export)
                })
        })
}

/// Scan the enum members without the modules that production mode excludes.
///
/// The scan uses an empty suppression context. It does not consume a
/// suppression of this run, and it reports each member that production mode
/// would report before suppressions apply.
fn find_production_unused_enum_members(input: &PostDetectionInput<'_, '_>) -> Vec<UnusedMember> {
    let production_modules: Vec<_> = input
        .resolved_modules
        .iter()
        .filter(|resolved| {
            input
                .graph
                .modules
                .get(resolved.file_id.0 as usize)
                .is_some_and(|module| !is_production_excluded(input.config, module))
        })
        .cloned()
        .collect();
    let no_suppressions = SuppressionContext::empty();
    find_unused_members_with_public_api_entry_points(UnusedMemberScanInput {
        graph: input.graph,
        resolved_modules: &production_modules,
        modules: input.modules,
        suppressions: &no_suppressions,
        line_offsets_by_file: input.line_offsets_by_file,
        user_class_member_allowlist: &[],
        semantic_framework_candidates: &[],
        ignore_decorators: &[],
        public_api_entry_points: input.public_api_entry_points,
        lit_active: false,
    })
    .enum_members
}

/// Whether a file that production mode keeps references `export`.
///
/// Without such a reference, production mode reports the export itself and
/// does not scan its members.
fn has_production_reference(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    export: &ExportSymbol,
) -> bool {
    export.references.iter().any(|reference| {
        graph
            .modules
            .get(reference.from_file.0 as usize)
            .is_some_and(|importer| !is_production_excluded(config, importer))
    })
}

/// Whether production mode would report an export that `suppression` covers.
///
/// Always `false` in production mode: that run consumes the suppression
/// directly when it is needed.
fn is_needed_in_production(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    file_id: FileId,
    suppression: &Suppression,
) -> bool {
    if config.production {
        return false;
    }
    let Some(module) = graph.modules.get(file_id.0 as usize) else {
        return false;
    };
    module.exports.iter().any(|export| {
        let (line, _) = byte_offset_to_line_col(line_offsets_by_file, file_id, export.span.start);
        let kind = if export.is_type_only {
            IssueKind::UnusedType
        } else {
            IssueKind::UnusedExport
        };
        suppression.matches_issue_kind(line, kind)
            && is_referenced_only_by_production_excluded_files(graph, config, export)
            && !is_re_exported_by_production_entry(graph, config, file_id, &export.name)
    })
}

/// Whether `export` has references and every one of them comes from a file
/// that production mode excludes from discovery.
fn is_referenced_only_by_production_excluded_files(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    export: &ExportSymbol,
) -> bool {
    !export.references.is_empty()
        && export.references.iter().all(|reference| {
            graph
                .modules
                .get(reference.from_file.0 as usize)
                .is_some_and(|importer| is_production_excluded(config, importer))
        })
}

/// Whether an entry point that production mode keeps re-exports the export,
/// directly or through a chain of barrels.
///
/// Production mode credits the exports of an entry-point barrel that has no
/// consumer in the graph, so such an export is used there.
fn is_re_exported_by_production_entry(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    file_id: FileId,
    name: &ExportName,
) -> bool {
    let start = match name {
        ExportName::Named(name) => name.clone(),
        ExportName::Default => "default".to_owned(),
    };
    let mut seen: FxHashSet<(FileId, String)> = FxHashSet::default();
    let mut pending = vec![(file_id, start)];
    while let Some((source, name)) = pending.pop() {
        if !seen.insert((source, name.clone())) {
            continue;
        }
        for barrel in &graph.modules {
            if is_production_excluded(config, barrel) {
                continue;
            }
            for edge in &barrel.re_exports {
                let forwards = edge.source_file == source
                    && (edge.imported_name == name
                        || (edge.imported_name == "*" && name != "default"));
                if !forwards {
                    continue;
                }
                if barrel.is_entry_point() {
                    return true;
                }
                let next = if edge.exported_name == "*" {
                    name.clone()
                } else {
                    edge.exported_name.clone()
                };
                pending.push((barrel.file_id, next));
            }
        }
    }
    false
}

/// Whether production mode leaves `module` out of discovery.
fn is_production_excluded(config: &ResolvedConfig, module: &ModuleNode) -> bool {
    let relative = module
        .path
        .strip_prefix(&config.root)
        .unwrap_or(&module.path);
    production_exclude_globset().is_some_and(|excludes| excludes.is_match(relative))
}
