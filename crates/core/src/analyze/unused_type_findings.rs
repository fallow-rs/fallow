//! The `unused-type` findings of a run, after the types that a same-file
//! signature keeps are removed.

use std::path::Path;

use fallow_config::{ResolvedConfig, Severity};
use fallow_types::output_dead_code::UnusedTypeFinding;
use fallow_types::trace::SignatureBackingReason;

#[path = "declaration_emit.rs"]
mod declaration_emit;

use super::unused_exports::suppress_signature_backing_types;
use crate::extract::ModuleInfo;
use crate::graph::ModuleGraph;
use crate::results::{AnalysisResults, UnusedExport};

pub(super) fn populate_unused_type_findings(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    types: Vec<UnusedExport>,
    unused_values: &[UnusedExport],
) {
    if config.rules.unused_types == Severity::Off {
        return;
    }
    let mut typed = types;
    let mut declaration_emit = declaration_emit::DeclarationEmit::new(&config.root);
    let mut keep_reason =
        |path: &Path| signature_backing_reason(config, &mut declaration_emit, path);
    results.signature_backed_types = suppress_signature_backing_types(
        &mut typed,
        unused_values,
        graph,
        modules,
        &mut keep_reason,
    );
    results.unused_types = typed
        .into_iter()
        .map(UnusedTypeFinding::with_actions)
        .collect();
}

/// Why a same-file signature use keeps a type export of `path`: the
/// `private-type-leak` rule is on for the file, or the project of the file
/// emits declarations. `None` when nothing needs the export (issue #5280).
fn signature_backing_reason(
    config: &ResolvedConfig,
    declaration_emit: &mut declaration_emit::DeclarationEmit,
    path: &Path,
) -> Option<SignatureBackingReason> {
    if config.rules.private_type_leaks != Severity::Off
        && config.resolve_rules_for_path(path).private_type_leaks != Severity::Off
    {
        return Some(SignatureBackingReason::PrivateTypeLeak);
    }
    declaration_emit
        .config_for(path)
        .map(|config| SignatureBackingReason::DeclarationEmit { config })
}
