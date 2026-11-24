//! The reason text of an export trace, and the visibility tag that keeps an
//! unused export out of `unused-export`.

use fallow_types::extract::VisibilityTag;
use fallow_types::trace::ReExportChain;

use crate::graph::{ExportNamespace, ModuleNode};

/// The kept tag and the reason text of the traced export `name`.
///
/// The analysis clears each tag that `keepExportTags` does not keep before
/// the graph build, so a tag on a graph export is a kept tag. The tag is the
/// reason for no finding only when the export has no consumer and its file is
/// reachable, is not an entry point, and has no star-export collision.
pub(super) fn export_verdict(
    module: &ModuleNode,
    name: (&str, ExportNamespace),
    reference_count: usize,
    re_export_chains: &[ReExportChain],
    ambiguous: bool,
) -> (Option<&'static str>, String) {
    if ambiguous {
        let reason = "Star re-export collision prevents consumers from resolving this declaration";
        return (None, reason.to_string());
    }
    let is_used = module.is_reachable() && reference_count > 0;
    let kept_by_tag = (!is_used && module.is_reachable() && !module.is_entry_point())
        .then(|| kept_visibility_tag(module, name))
        .flatten();
    let reason = export_reason(
        module,
        reference_count,
        is_used,
        re_export_chains,
        kept_by_tag,
    );
    (kept_by_tag, reason)
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
