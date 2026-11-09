//! Bounded architecture inventory joined through unique value declarations.

use fallow_config::ResolvedConfig;
use fallow_graph::graph::{ExportNamespace, ModuleGraph};
use fallow_graph::resolve::ResolvedModule;
use fallow_types::extract::{ImportedName, ModuleInfo, SemanticFact, byte_offset_to_line_col};
use fallow_types::shared_state::*;
use fallow_types::suppress::{IssueKind, is_suppressed};
use rustc_hash::FxHashMap;
use std::collections::BTreeMap;
use std::path::Path;

const MAX_INVENTORY_SITES: usize = 20_000;
#[path = "shared_state_builtins.rs"]
mod builtins;

pub fn inventory(
    config: &ResolvedConfig,
    modules: &[ModuleInfo],
    resolved: &[ResolvedModule],
    graph: &ModuleGraph,
) -> Option<SharedStateInventory> {
    if config.extraction_mode != ExtractionMode::SharedState {
        return None;
    }
    let parsed: FxHashMap<_, _> = modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();
    let paths: FxHashMap<_, _> = resolved
        .iter()
        .map(|module| (module.file_id, module.path.as_path()))
        .collect();
    let origins = OriginIndex {
        parsed: &parsed,
        paths: &paths,
        graph,
        root: &config.root,
    };
    let mut sorted: Vec<_> = resolved.iter().collect();
    sorted.sort_by(|left, right| left.path.cmp(&right.path));
    let mut output = SharedStateInventory {
        version: 1,
        complete: modules.iter().all(|module| {
            module.parse_error_count == 0
                && !module.parse_panicked
                && !module.parse_nesting_too_deep
        }),
        truncated: false,
        imported_writes: Vec::new(),
        globals: Vec::new(),
    };
    let mut globals = BTreeMap::<(String, String), GlobalStateGroup>::new();
    let mut retained = 0;
    for module in sorted {
        let Some(info) = parsed.get(&module.file_id) else {
            output.complete = false;
            continue;
        };
        let relative = module
            .path
            .strip_prefix(&config.root)
            .unwrap_or(&module.path);
        if crate::test_paths::is_test_path(&config.root, relative) {
            continue;
        }
        for fact in info.semantic_facts.iter() {
            if matches!(fact, SemanticFact::SharedStateTruncated) {
                output.truncated = true;
            }
            let SemanticFact::SharedState(fact) = fact else {
                continue;
            };
            let site = site(&module.path, info, fact.span_start);
            if is_suppressed(&info.suppressions, site.line as u32, IssueKind::SharedState)
                || config
                    .ignore_findings
                    .is_ignored_path(&config.root, &module.path)
            {
                continue;
            }
            if retained >= MAX_INVENTORY_SITES {
                output.truncated = true;
                continue;
            }
            retained += 1;
            match &fact.root {
                StateRoot::Import { import_index } => {
                    if let Some(write) = imported_write(module, *import_index, fact, site, &origins)
                    {
                        output.imported_writes.push(write);
                    }
                }
                StateRoot::Global { name } => add_global(&mut globals, name, fact, site, config),
            }
        }
    }
    output.globals = globals.into_values().filter_map(finish_global).collect();
    output.imported_writes.sort_by(|left, right| {
        left.writer
            .cmp(&right.writer)
            .then_with(|| left.id.cmp(&right.id))
    });
    output.complete &= !output.truncated;
    Some(output)
}

/// Include recorded source loss even when the parser returned no module.
pub fn sources_complete(
    config: &ResolvedConfig,
    discovery: &crate::discover::AnalysisDiscovery,
) -> bool {
    let recorded = fallow_config::registry_diagnostics_to_fold(&config.root);
    discovery.source_diagnostics().iter().chain(&recorded).all(|diagnostic| {
        !diagnostic.kind.source_never_analyzed()
            && !matches!(diagnostic.kind,
                fallow_types::workspace::WorkspaceDiagnosticKind::SourceParseDegraded { .. }
                | fallow_types::workspace::WorkspaceDiagnosticKind::SourceNestingTooDeep { .. })
    })
}

fn site(path: &Path, module: &ModuleInfo, offset: u32) -> StateSite {
    let (line, column) = byte_offset_to_line_col(&module.line_offsets, offset);
    StateSite {
        path: path.to_owned(),
        line: line as usize,
        column: column as usize + 1,
        offset,
    }
}

struct OriginIndex<'a> {
    parsed: &'a FxHashMap<fallow_types::discover::FileId, &'a ModuleInfo>,
    paths: &'a FxHashMap<fallow_types::discover::FileId, &'a Path>,
    graph: &'a ModuleGraph,
    root: &'a Path,
}

fn imported_write(
    module: &ResolvedModule,
    index: u32,
    fact: &StateAccessFact,
    writer: StateSite,
    origins: &OriginIndex<'_>,
) -> Option<ImportedStateWrite> {
    let root = origins.root;
    let import = module.resolved_imports.get(index as usize)?;
    if import.info.is_type_only {
        return None;
    }
    let target = import.target.internal_file_id()?;
    let (name, property) = match &import.info.imported_name {
        ImportedName::Named(name) => (name.as_str(), fact.path.as_slice()),
        ImportedName::Default => ("default", fact.path.as_slice()),
        ImportedName::Namespace if fact.path.len() > 1 => (fact.path[0].as_str(), &fact.path[1..]),
        _ => return None,
    };
    let origin = origins
        .graph
        .resolve_export_origin(target, name, ExportNamespace::Value)?;
    if origin.file_id() == module.file_id {
        return None;
    }
    let owner = *origins.paths.get(&origin.file_id())?;
    let relative = owner.strip_prefix(root).unwrap_or(owner);
    if crate::test_paths::is_test_path(root, relative) {
        return None;
    }
    let declaration = site(
        owner,
        origins.parsed.get(&origin.file_id())?,
        origin.export().span.start,
    );
    let binding = origin.export().name.to_string();
    let mut identity_parts = vec![
        fallow_types::identity::IdentityPaths::new(root).key(&writer.path),
        writer.offset.to_string(),
        fallow_types::identity::IdentityPaths::new(root).key(owner),
        declaration.offset.to_string(),
        binding.clone(),
        fact.operation.label().to_owned(),
    ];
    identity_parts.extend(property.iter().cloned());
    let refs: Vec<_> = identity_parts.iter().map(String::as_str).collect();
    let id = identity("import", &refs);
    Some(ImportedStateWrite {
        id,
        writer,
        declaration,
        binding,
        property: property.to_vec(),
        operation: fact.operation,
        actions: actions(),
    })
}

fn add_global(
    groups: &mut BTreeMap<(String, String), GlobalStateGroup>,
    root: &str,
    fact: &StateAccessFact,
    site: StateSite,
    config: &ResolvedConfig,
) {
    let Some(slot) = fact.path.first() else {
        return;
    };
    if builtins::is_builtin(root, slot)
        || config
            .shared_state_allowlist
            .iter()
            .any(|key| key[0] == root && key[1] == *slot)
    {
        return;
    }
    let key = (root.to_owned(), slot.clone());
    let group = groups
        .entry(key.clone())
        .or_insert_with(|| GlobalStateGroup {
            id: identity("global", &[&key.0, &key.1]),
            root: key.0,
            slot: key.1,
            reads: Vec::new(),
            writes: Vec::new(),
            same_runtime: UnknownStateRelation::Unknown,
            execution_order: UnknownStateRelation::Unknown,
            actions: actions(),
        });
    let access = GlobalStateAccess {
        site,
        property: fact.path.clone(),
        operation: fact.operation,
    };
    if matches!(
        fact.operation,
        StateOperation::Read | StateOperation::ReadModifyWrite | StateOperation::Update
    ) {
        group.reads.push(access.clone());
    }
    if fact.operation != StateOperation::Read {
        group.writes.push(access);
    }
}

fn finish_global(mut group: GlobalStateGroup) -> Option<GlobalStateGroup> {
    let first_read = group.reads.first()?;
    let first_write = group.writes.first()?;
    let cross_file = first_read.site.path != first_write.site.path
        || group
            .reads
            .iter()
            .any(|read| read.site.path != first_write.site.path)
        || group
            .writes
            .iter()
            .any(|write| write.site.path != first_read.site.path);
    if !cross_file {
        return None;
    }
    group.reads.sort();
    group.reads.dedup();
    group.writes.sort();
    group.writes.dedup();
    Some(group)
}

fn actions() -> Vec<StateAction> {
    vec![StateAction { description: "Review ownership and initialization; suppress intentional sites with fallow-ignore-next-line shared-state.".to_owned(), auto_fixable: false }]
}

fn identity(kind: &str, parts: &[&str]) -> String {
    format!(
        "ss1:{kind}:{}",
        fallow_types::identity::fnv1a64_parts(parts)
    )
}
