//! `duplicate-export-alias`: one module exports one binding under two or more
//! names, for example `export const a = 1; export default a;`.
//!
//! The rule reads the export surface of each file. It groups exports by the
//! binding that they export, and reports a group that has two or more
//! distinct export names. The grouping keys are:
//!
//! - a local export: the local binding name and the type-only flag. A value
//!   export and a type-only export of one name are separate groups, because
//!   TypeScript lets a value and a type share one name. A plain specifier
//!   (`export { I as J }`) of a name that the file declares only as a type
//!   joins the type-only group of that name.
//! - a named re-export: the resolved source file (or the raw specifier when
//!   the source does not resolve to a project file), the imported name and
//!   the type-only flag.
//!
//! Two bindings with the same initializer have two local names, so they are
//! never one group. `export *` exports no single binding and is skipped. A
//! module with CommonJS exports (`export =`, `module.exports`) is skipped,
//! because its default export is the whole module object.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::ResolvedConfig;
use fallow_types::extract::ExportName;

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::plugins::is_export_ignored;
use crate::resolve::ResolvedModule;
use crate::results::{DuplicateExportAlias, DuplicateExportAliasLocation};
use crate::suppress::{IssueKind, SuppressionContext};

use super::unused_exports::ignore_matchers_for_module;
use super::{LineOffsetsMap, byte_offset_to_line_col};

/// File extensions whose export surface the rule reads. Component files
/// (`.vue`, `.svelte`, `.astro`) give `export` a framework meaning, so the
/// rule leaves them out.
const SCRIPT_EXTENSIONS: &[&str] = &["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"];

/// The binding that one export statement exports.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum BindingKey {
    Local {
        name: String,
        is_type_only: bool,
    },
    ReExport {
        source: ReExportSource,
        imported_name: String,
        is_type_only: bool,
    },
}

impl BindingKey {
    fn local_name(&self) -> &str {
        match self {
            Self::Local { name, .. } => name,
            Self::ReExport { imported_name, .. } => imported_name,
        }
    }
}

/// The module that a named re-export reads from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ReExportSource {
    File(FileId),
    Specifier(String),
}

/// One export name of a binding, before line and column resolution.
struct AliasEntry {
    export_name: String,
    span_start: u32,
}

/// Find the bindings that one module exports under two or more names.
pub(super) fn find_duplicate_export_aliases(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    resolved_modules: &[ResolvedModule],
) -> Vec<DuplicateExportAlias> {
    let mut findings = Vec::new();
    for module in resolved_modules {
        if !is_candidate_module(graph, module) {
            continue;
        }
        if suppressions.is_file_suppressed(module.file_id, IssueKind::DuplicateExportAlias) {
            continue;
        }
        let matching_ignore = ignore_matchers_for_module(
            &module.path,
            &config.root,
            config.compiled_ignore_exports.as_slice(),
        );
        if matching_ignore
            .iter()
            .any(|exports| exports.iter().any(|name| name == "*"))
        {
            continue;
        }

        for (key, entries) in group_module_exports(module) {
            if entries
                .iter()
                .any(|entry| is_export_ignored(&entry.export_name, &matching_ignore, &[]))
            {
                continue;
            }
            if let Some(finding) =
                build_finding(module, &key, &entries, suppressions, line_offsets_by_file)
            {
                findings.push(finding);
            }
        }
    }
    findings
}

/// Whether the rule reads the export surface of this module.
fn is_candidate_module(graph: &ModuleGraph, module: &ResolvedModule) -> bool {
    if module.has_cjs_exports {
        return false;
    }
    let is_script = module
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| SCRIPT_EXTENSIONS.contains(&extension));
    if !is_script {
        return false;
    }
    graph
        .modules
        .get(module.file_id.0 as usize)
        .is_some_and(|node| node.file_id == module.file_id && node.is_reachable())
}

/// Group the exports of one module by binding, in source order. Each group
/// keeps one entry per distinct export name. Only groups with two or more
/// names are returned.
fn group_module_exports(module: &ResolvedModule) -> Vec<(BindingKey, Vec<AliasEntry>)> {
    let mut order: Vec<BindingKey> = Vec::new();
    let mut groups: FxHashMap<BindingKey, Vec<AliasEntry>> = FxHashMap::default();
    let mut push = |key: BindingKey, export_name: String, span_start: u32| {
        let entries = groups.entry(key.clone()).or_insert_with(|| {
            order.push(key);
            Vec::new()
        });
        if entries.iter().all(|entry| entry.export_name != export_name) {
            entries.push(AliasEntry {
                export_name,
                span_start,
            });
        }
    };

    let type_only_declarations = type_only_declaration_names(module);
    for export in module.exports.iter() {
        if export.span.start == 0 && export.span.end == 0 {
            continue;
        }
        let export_name = export.name.to_string();
        let name = match (&export.local_name, &export.name) {
            (Some(local), _) => local.clone(),
            (None, ExportName::Named(name)) => name.clone(),
            // An anonymous `export default <expression>` has no binding.
            (None, ExportName::Default) => continue,
        };
        let is_type_only = export.is_type_only || type_only_declarations.contains(name.as_str());
        push(
            BindingKey::Local { name, is_type_only },
            export_name,
            export.span.start,
        );
    }

    for re_export in &module.re_exports {
        let info = &re_export.info;
        if info.exported_name == "*" || (info.span.start == 0 && info.span.end == 0) {
            continue;
        }
        push(
            BindingKey::ReExport {
                source: re_export.target.internal_file_id().map_or_else(
                    || ReExportSource::Specifier(info.source.clone()),
                    ReExportSource::File,
                ),
                imported_name: info.imported_name.clone(),
                is_type_only: info.is_type_only,
            },
            info.exported_name.clone(),
            info.span.start,
        );
    }

    order
        .into_iter()
        .filter_map(|key| {
            let mut entries = groups.remove(&key)?;
            if entries.len() < 2 {
                return None;
            }
            entries.sort_by_key(|entry| entry.span_start);
            Some((key, entries))
        })
        .collect()
}

/// Names that the module declares and exports only as a type
/// (`export interface I {}`, `export type T = ...`). A plain specifier of such
/// a name exports the same type binding.
fn type_only_declaration_names(module: &ResolvedModule) -> FxHashSet<&str> {
    let mut type_names = FxHashSet::default();
    let mut value_names = FxHashSet::default();
    for export in module.exports.iter() {
        let ExportName::Named(name) = &export.name else {
            continue;
        };
        if export
            .local_name
            .as_ref()
            .is_some_and(|local| local != name)
        {
            continue;
        }
        if export.is_type_only {
            type_names.insert(name.as_str());
        } else {
            value_names.insert(name.as_str());
        }
    }
    type_names.retain(|name| !value_names.contains(name));
    type_names
}

fn build_finding(
    module: &ResolvedModule,
    key: &BindingKey,
    entries: &[AliasEntry],
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Option<DuplicateExportAlias> {
    let locations: Vec<DuplicateExportAliasLocation> = entries
        .iter()
        .map(|entry| {
            let (line, col) =
                byte_offset_to_line_col(line_offsets_by_file, module.file_id, entry.span_start);
            DuplicateExportAliasLocation {
                export_name: entry.export_name.clone(),
                line,
                col,
            }
        })
        .collect();
    if is_any_line_suppressed(suppressions, module.file_id, &locations) {
        return None;
    }
    let first = locations.first()?;
    Some(DuplicateExportAlias {
        path: module.path.clone(),
        local_name: key.local_name().to_string(),
        export_names: locations
            .iter()
            .map(|location| location.export_name.clone())
            .collect(),
        line: first.line,
        col: first.col,
        locations,
    })
}

/// A suppression comment above any export name of the binding hides the
/// finding. Each line is checked once, so every matching comment counts as
/// used and none becomes stale.
fn is_any_line_suppressed(
    suppressions: &SuppressionContext<'_>,
    file_id: FileId,
    locations: &[DuplicateExportAliasLocation],
) -> bool {
    let lines: FxHashSet<u32> = locations.iter().map(|location| location.line).collect();
    let mut suppressed = false;
    for line in lines {
        suppressed |= suppressions.is_suppressed(file_id, line, IssueKind::DuplicateExportAlias);
    }
    suppressed
}
