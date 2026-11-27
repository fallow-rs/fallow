//! The reason text of an export trace, and the visibility tag that keeps an
//! unused export out of `unused-export`.

use fallow_types::extract::VisibilityTag;
use fallow_types::trace::ReExportChain;

use crate::graph::{ExportNamespace, ModuleNode};

/// The credit that the trace found for one export.
#[derive(Clone, Copy)]
pub(super) struct ExportCredit {
    /// Distinct reachable files that reference the export.
    pub(super) reference_count: usize,
    /// Whether the export is the implicit default of a component file. The
    /// unused-export analysis never reports this default: an unused
    /// component is an unused file.
    pub(super) implicit_component_default: bool,
}

/// The verdict of an export trace.
pub(super) struct ExportVerdict {
    pub(super) is_used: bool,
    pub(super) kept_by_tag: Option<&'static str>,
    pub(super) reason: String,
}

/// The verdict, the kept tag and the reason text of the traced export `name`.
///
/// The analysis clears each tag that `keepExportTags` does not keep before
/// the graph build, so a tag on a graph export is a kept tag. The tag is the
/// reason for no finding only when the export has no consumer and its file is
/// reachable, is not an entry point, and has no star-export collision.
pub(super) fn export_verdict(
    module: &ModuleNode,
    name: (&str, ExportNamespace),
    credit: ExportCredit,
    re_export_chains: &[ReExportChain],
    ambiguous: bool,
) -> ExportVerdict {
    let is_used =
        module.is_reachable() && (credit.reference_count > 0 || credit.implicit_component_default);
    if ambiguous {
        let reason = "Star re-export collision prevents consumers from resolving this declaration";
        return ExportVerdict {
            is_used,
            kept_by_tag: None,
            reason: reason.to_string(),
        };
    }
    let kept_by_tag = (!is_used && module.is_reachable() && !module.is_entry_point())
        .then(|| kept_visibility_tag(module, name))
        .flatten();
    let reason = export_reason(
        module,
        credit.reference_count,
        is_used,
        re_export_chains,
        kept_by_tag,
    );
    ExportVerdict {
        is_used,
        kept_by_tag,
        reason,
    }
}

/// The kept visibility tag of the export, preferring the export of the
/// traced namespace when a value and a type share the name.
fn kept_visibility_tag(
    module: &ModuleNode,
    (export_name, namespace): (&str, ExportNamespace),
) -> Option<&'static str> {
    let mut named = module
        .exports
        .iter()
        .filter(|export| export.name.matches_str(export_name));
    let in_namespace = named
        .clone()
        .find(|export| export.is_type_only == (namespace == ExportNamespace::Type));
    let export = in_namespace.or_else(|| named.next())?;
    match export.visibility {
        VisibilityTag::Public => Some("public"),
        VisibilityTag::Internal => Some("internal"),
        VisibilityTag::Alpha => Some("alpha"),
        VisibilityTag::Beta => Some("beta"),
        VisibilityTag::None | VisibilityTag::ExpectedUnused => None,
    }
}

/// Build the human-readable reason string explaining an export's used/unused state.
fn export_reason(
    module: &ModuleNode,
    reference_count: usize,
    is_used: bool,
    re_export_chains: &[ReExportChain],
    kept_by_tag: Option<&str>,
) -> String {
    if !module.is_reachable() {
        "File is unreachable from any entry point".to_string()
    } else if is_used && reference_count == 0 {
        "No file imports the component by its default, but the component file is reachable, \
         and dead-code reports an unused component as an unused file"
            .to_string()
    } else if is_used {
        format!(
            "Used by {} file(s){}",
            reference_count,
            if re_export_chains.is_empty() {
                String::new()
            } else {
                format!(", re-exported through {} barrel(s)", re_export_chains.len())
            }
        )
    } else if module.is_entry_point() {
        "No internal references, but file is an entry point (export is externally accessible)"
            .to_string()
    } else if let Some(tag) = kept_by_tag {
        format!("No references found, but the @{tag} tag keeps the export (keepExportTags)")
    } else if !re_export_chains.is_empty() {
        format!(
            "Re-exported through {} barrel(s) but no consumer imports it through the barrel",
            re_export_chains.len()
        )
    } else {
        "No references found, export is unused".to_string()
    }
}

/// The star-export collision that makes `export_name` of `file_id`
/// ambiguous, when one exists.
pub(super) fn trace_star_export_ambiguity(
    graph: &crate::graph::ModuleGraph,
    root: &std::path::Path,
    file_id: crate::discover::FileId,
    export_name: &str,
) -> Option<fallow_types::trace_chain::StarExportAmbiguity> {
    let collisions: Vec<_> = graph
        .ambiguous_star_exports()
        .into_iter()
        .filter(|collision| {
            collision.name.as_ref() == export_name
                && (collision.barrel == file_id || collision.contributors.contains(&file_id))
        })
        .collect();
    if collisions.is_empty() {
        return None;
    }
    let mut sources: Vec<_> = collisions
        .iter()
        .flat_map(|collision| collision.contributors.iter())
        .filter_map(|contributor| graph.module(*contributor))
        .map(|module| super::trace_impl::relative_module_path(&module.path, root))
        .collect();
    sources.sort();
    sources.dedup();
    let mut namespaces: Vec<_> = collisions
        .iter()
        .map(|collision| match collision.namespace {
            ExportNamespace::Type => fallow_types::semantic::SemanticNamespace::Type,
            ExportNamespace::Value => fallow_types::semantic::SemanticNamespace::Value,
        })
        .collect();
    namespaces.sort_unstable_by_key(|namespace| match namespace {
        fallow_types::semantic::SemanticNamespace::Type => 0,
        fallow_types::semantic::SemanticNamespace::Value => 1,
    });
    namespaces.dedup();
    Some(fallow_types::trace_chain::StarExportAmbiguity {
        sources,
        namespaces,
    })
}
