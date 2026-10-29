//! Keep a suppression that only production mode needs out of the stale list.
//!
//! Production mode excludes test, story, and fixture files from discovery. An
//! export that only those files import is unused under `--production`, so its
//! `unused-export` or `unused-type` suppression is needed there. A run without
//! `--production` credits the test import and does not consume the
//! suppression. Reporting it stale there would leave no comment that is clean
//! in both modes (issue #3825).
//!
//! The same applies to an `unused-enum-member`, `unused-class-member`, or
//! `unused-store-member` suppression on a member that only those files read
//! (issue #3962).

use std::cell::OnceCell;

use fallow_config::{ResolvedConfig, Severity};
use fallow_types::extract::ExportName;
use fallow_types::suppress::SuppressionTarget;
use rustc_hash::FxHashSet;

use crate::discover::FileId;
use crate::graph::{ExportSymbol, ModuleGraph, ModuleNode};
use crate::results::{StaleSuppression, UnusedMember};
use crate::suppress::{IssueKind, Suppression, SuppressionContext};

use super::members::{
    UnusedMemberResults, UnusedMemberScanInput, find_unused_members_with_public_api_entry_points,
};
use super::unused_deps::production_exclude_globset;
use super::{
    LineOffsetsMap, PostDetectionInput, byte_offset_to_line_col, lit_is_declared,
    store_member_rule_is_active,
};

const MEMBER_KINDS: [IssueKind; 3] = [
    IssueKind::UnusedEnumMember,
    IssueKind::UnusedClassMember,
    IssueKind::UnusedStoreMember,
];

/// Collect the stale suppressions of this run, without a suppression that
/// only production mode needs.
pub(super) fn find_stale(input: &PostDetectionInput<'_, '_>) -> Vec<StaleSuppression> {
    let (graph, config) = (input.graph, input.config);
    let production_members: OnceCell<ProductionMemberView> = OnceCell::new();
    input
        .suppressions
        .find_stale_except(graph, config, |file_id, suppression| {
            is_needed_in_production(
                graph,
                config,
                input.line_offsets_by_file,
                file_id,
                suppression,
            ) || is_member_needed_in_production(input, file_id, suppression, &production_members)
        })
}

/// Whether production mode would report a member that `suppression` covers.
///
/// Production mode excludes a test file, so it does not credit a member
/// access in that file. The check scans the members again without the
/// accesses of production-excluded files. The scan runs once, and only when
/// an unconsumed suppression can cover a member.
fn is_member_needed_in_production(
    input: &PostDetectionInput<'_, '_>,
    file_id: FileId,
    suppression: &Suppression,
    production_members: &OnceCell<ProductionMemberView>,
) -> bool {
    let (graph, config) = (input.graph, input.config);
    if config.production {
        return false;
    }
    let mut kinds = MEMBER_KINDS
        .into_iter()
        .filter(|&kind| targets_kind(suppression, kind) && is_member_rule_active(input, kind))
        .peekable();
    if kinds.peek().is_none() {
        return false;
    }
    let Some(module) = graph.modules.get(file_id.0 as usize) else {
        return false;
    };
    if is_production_excluded(config, module)
        || module
            .exports
            .iter()
            .all(|export| export.members.is_empty())
    {
        return false;
    }
    let view = production_members.get_or_init(|| ProductionMemberView::new(input));
    if !view
        .reachable
        .get(file_id.0 as usize)
        .copied()
        .unwrap_or(false)
    {
        return false;
    }
    kinds.any(|kind| {
        unused_members_of_kind(&view.unused, kind).any(|member| {
            member.path == module.path
                && suppression.matches_issue_kind(member.line, kind)
                && module.exports.iter().any(|export| {
                    export.name.to_string() == member.parent_name
                        && has_production_reference(graph, config, export)
                })
        })
    })
}

/// The members that production mode would scan, computed once per run.
struct ProductionMemberView {
    /// The members that the scan without production-excluded files reports.
    unused: UnusedMemberResults,
    /// The modules that production mode can reach, indexed by `FileId`.
    reachable: Vec<bool>,
}

impl ProductionMemberView {
    fn new(input: &PostDetectionInput<'_, '_>) -> Self {
        Self {
            unused: find_production_unused_members(input),
            reachable: production_reachable_modules(input.graph, input.config),
        }
    }
}

/// The modules that an entry point reaches without a production-excluded
/// module on the path.
///
/// Production mode does not scan the members of a module that only a test
/// reaches. It reports the module as an unused file. The walk starts from
/// every entry point that production mode keeps, so the set can be larger
/// than the production set but not smaller.
fn production_reachable_modules(graph: &ModuleGraph, config: &ResolvedConfig) -> Vec<bool> {
    let kept = |module: &ModuleNode| !is_production_excluded(config, module);
    let mut reachable = vec![false; graph.modules.len()];
    let mut pending: Vec<FileId> = graph
        .modules
        .iter()
        .filter(|module| module.is_entry_point() && kept(module))
        .map(|module| module.file_id)
        .collect();
    while let Some(file_id) = pending.pop() {
        let Some(seen) = reachable.get_mut(file_id.0 as usize) else {
            continue;
        };
        if std::mem::replace(seen, true) {
            continue;
        }
        pending.extend(graph.edges_for(file_id).into_iter().filter(|target| {
            !reachable.get(target.0 as usize).copied().unwrap_or(true)
                && graph.modules.get(target.0 as usize).is_some_and(kept)
        }));
    }
    reachable
}

/// Whether `suppression` names `kind` or covers every issue kind.
fn targets_kind(suppression: &Suppression, kind: IssueKind) -> bool {
    match &suppression.target {
        None => true,
        Some(SuppressionTarget::Issue(target)) => *target == kind,
        Some(SuppressionTarget::PolicyRule(_)) => false,
    }
}

/// Whether the run reports members of `kind` at all.
fn is_member_rule_active(input: &PostDetectionInput<'_, '_>, kind: IssueKind) -> bool {
    let rules = &input.config.rules;
    match kind {
        IssueKind::UnusedEnumMember => rules.unused_enum_members != Severity::Off,
        IssueKind::UnusedClassMember => rules.unused_class_members != Severity::Off,
        IssueKind::UnusedStoreMember => {
            store_member_rule_is_active(input.config, input.declared_deps)
        }
        _ => false,
    }
}

/// The members of `kind` that the production scan reports.
///
/// A class member that only the semantic pass can confirm counts as reported,
/// so the check keeps its suppression rather than call it stale.
fn unused_members_of_kind(
    members: &UnusedMemberResults,
    kind: IssueKind,
) -> Box<dyn Iterator<Item = &UnusedMember> + '_> {
    match kind {
        IssueKind::UnusedEnumMember => Box::new(members.enum_members.iter()),
        IssueKind::UnusedClassMember => Box::new(
            members
                .class_members
                .iter()
                .map(|candidate| &candidate.member),
        ),
        IssueKind::UnusedStoreMember => Box::new(members.store_members.iter()),
        _ => Box::new(std::iter::empty()),
    }
}

/// Scan the members without the modules that production mode excludes.
///
/// The scan uses the class-member rules of the run and an empty suppression
/// context. It does not consume a suppression of this run, and it reports each
/// member that production mode would report before suppressions apply.
fn find_production_unused_members(input: &PostDetectionInput<'_, '_>) -> UnusedMemberResults {
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
        user_class_member_allowlist: &input.class_member_rules.user,
        semantic_framework_candidates: &input.class_member_rules.semantic_framework_candidates,
        ignore_decorators: &input.config.ignore_decorators,
        public_api_entry_points: input.public_api_entry_points,
        lit_active: lit_is_declared(input.declared_deps),
        report_unmatched_patterns: false,
    })
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
