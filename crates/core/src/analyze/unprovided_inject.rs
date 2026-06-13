//! Detection of dead dependency-injection links: a Vue `inject(KEY)` or Svelte
//! `getContext(KEY)` whose symbol KEY is `provide`/`setContext`'d nowhere in the
//! analyzed project (the injected-never-provided direction).
//!
//! The key is a symbol with cross-file identity (an imported const or a
//! module-local symbol), so an unmatched key is a real dead-half: at runtime the
//! inject returns `undefined`, surfaced only at render time. No static tool in
//! the Vue/Svelte/Nuxt ecosystems catches this (they emit runtime-only warnings
//! or have unimplemented eslint proposals).
//!
//! The detector is built to never false-flag (degrade by abstaining):
//! - **Dep-gated** on `vue` / `@vue/runtime-core` / `svelte`.
//! - **External-abstain**: a key imported from an npm PACKAGE is skipped, because
//!   the `provide` may live inside that package's own code (in `node_modules`,
//!   which fallow does not parse).
//! - **Public-API abstain**: a key that is part of this package's public API
//!   (re-exported from, or defined in, a non-private package entry point) is
//!   skipped, because a "bring-your-own-provider" library exports the key and an
//!   inject-composable for a downstream consumer to provide.
//! - **Dynamic-provide abstain**: if ANY reachable module provides a key fallow
//!   cannot pin to a stable symbol (a spread, a computed key, or a transient
//!   loop/parameter local), the whole project abstains, because a surviving
//!   inject finding could be falsely flagged. Mirrors the Pinia spread-return
//!   whole-object abstain.
//!
//! The provided set is built LIBERALLY (the composable `provide(KEY, _)` plus
//! app-level `*.provide(KEY, _)`): over-crediting a provided key can only
//! suppress a finding, never create one. The inject side emits conservatively.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{DiFramework, DiRole, ExportName, ModuleInfo};

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::resolve::ResolvedModule;
use crate::results::UnprovidedInject;
use crate::suppress::{IssueKind, SuppressionContext};

use super::unused_members::{
    ExportKey, build_local_to_export_keys, entry_point_star_re_export_targets,
    export_has_entry_point_re_export_reference, export_key_with_origins,
};
use super::{LineOffsetsMap, byte_offset_to_line_col};

/// How an injected/provided key identifier resolves to a cross-file identity.
enum KeyResolution {
    /// Resolved to internal defining-site export keys (barrel chains expanded).
    Internal(Vec<ExportKey>),
    /// Imported from an npm package (external target): abstain, the provide may
    /// live inside that package's own code.
    External,
    /// A non-exported module-local symbol: identity is `(file, name)`.
    LocalOnly(Vec<ExportKey>),
}

/// Find Vue `inject(KEY)` / Svelte `getContext(KEY)` calls whose symbol KEY is
/// provided nowhere in the analyzed project.
///
/// Returns empty unless the project declares `vue` / `@vue/runtime-core` /
/// `svelte`, or if any reachable module has an unknowable-key provide (see the
/// module docs for the abstain ladder).
#[must_use]
pub fn find_unprovided_injects(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules: &[ModuleInfo],
    declared_deps: &FxHashSet<String>,
    public_api_entry_points: &FxHashSet<FileId>,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<UnprovidedInject> {
    let vue = declared_deps.contains("vue") || declared_deps.contains("@vue/runtime-core");
    let svelte = declared_deps.contains("svelte");
    if !vue && !svelte {
        return Vec::new();
    }

    // Dynamic-provide abstain: a single unknowable-key provide anywhere means a
    // surviving inject finding could be a false positive, so abstain wholesale.
    if modules.iter().any(|module| module.has_dynamic_provide) {
        return Vec::new();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();
    let path_by_id: FxHashMap<FileId, &std::path::Path> = graph
        .modules
        .iter()
        .map(|module| (module.file_id, module.path.as_path()))
        .collect();

    // Pass 1: build the provided-key set liberally.
    let mut provided: FxHashSet<ExportKey> = FxHashSet::default();
    for resolved in resolved_modules {
        let Some(module) = modules_by_id.get(&resolved.file_id) else {
            continue;
        };
        if module
            .di_key_sites
            .iter()
            .all(|site| site.role != DiRole::Provide)
        {
            continue;
        }
        let local_to_export_keys = build_local_to_export_keys(resolved);
        for site in &module.di_key_sites {
            if site.role != DiRole::Provide {
                continue;
            }
            match resolve_key(resolved, graph, &local_to_export_keys, &site.key_local) {
                KeyResolution::Internal(keys) | KeyResolution::LocalOnly(keys) => {
                    provided.extend(keys);
                }
                KeyResolution::External => {}
            }
        }
    }

    let entry_star_targets = entry_point_star_re_export_targets(graph, public_api_entry_points);

    // Pass 2: emit a finding for each inject whose key is provided nowhere.
    let mut findings = Vec::new();
    for resolved in resolved_modules {
        let Some(module) = modules_by_id.get(&resolved.file_id) else {
            continue;
        };
        if module
            .di_key_sites
            .iter()
            .all(|site| site.role != DiRole::Inject)
        {
            continue;
        }
        let local_to_export_keys = build_local_to_export_keys(resolved);
        for site in &module.di_key_sites {
            if site.role != DiRole::Inject {
                continue;
            }
            let canonical =
                match resolve_key(resolved, graph, &local_to_export_keys, &site.key_local) {
                    // External: the provide may live inside the package; abstain.
                    KeyResolution::External => continue,
                    KeyResolution::Internal(keys) | KeyResolution::LocalOnly(keys) => keys,
                };
            if canonical.is_empty() {
                continue;
            }
            // Matched by a provide somewhere in the project.
            if canonical.iter().any(|key| provided.contains(key)) {
                continue;
            }
            // Public-API abstain: the consumer of this package provides the key.
            if canonical.iter().any(|key| {
                key_is_public_api(graph, key, public_api_entry_points, &entry_star_targets)
            }) {
                continue;
            }

            let (line, col) =
                byte_offset_to_line_col(line_offsets_by_file, resolved.file_id, site.span_start);
            if suppressions.is_suppressed(resolved.file_id, line, IssueKind::UnprovidedInject)
                || suppressions.is_file_suppressed(resolved.file_id, IssueKind::UnprovidedInject)
            {
                continue;
            }
            let Some(path) = path_by_id.get(&resolved.file_id) else {
                continue;
            };
            findings.push(UnprovidedInject {
                path: path.to_path_buf(),
                key_name: site.key_local.clone(),
                framework: framework_str(site.framework).to_string(),
                line,
                col,
            });
        }
    }

    findings
}

/// Resolve a key identifier to its cross-file identity, distinguishing an
/// internal symbol (resolvable to local defining sites) from a package import
/// (abstain) and a non-exported module-local symbol.
fn resolve_key(
    resolved: &ResolvedModule,
    graph: &ModuleGraph,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    key_local: &str,
) -> KeyResolution {
    if let Some(keys) = local_to_export_keys.get(key_local) {
        let mut canonical: Vec<ExportKey> = Vec::new();
        for key in keys {
            for origin in export_key_with_origins(graph, key) {
                if !canonical.contains(&origin) {
                    canonical.push(origin);
                }
            }
        }
        return KeyResolution::Internal(canonical);
    }

    // Not an internal import nor a local export: either an external package
    // import (abstain) or a purely-local non-exported symbol.
    let imported_external = resolved.all_resolved_imports().any(|import| {
        import.info.local_name == key_local && import.target.internal_file_id().is_none()
    });
    if imported_external {
        return KeyResolution::External;
    }
    KeyResolution::LocalOnly(vec![ExportKey::new(
        resolved.file_id,
        key_local.to_string(),
    )])
}

/// Whether the key's resolved export is part of this package's public API:
/// re-exported from, defined in, or reachable via `export *` from a non-private
/// package entry point. Such a key is provided by a downstream CONSUMER, so an
/// in-repo inject with no local provide is intentional, not dead.
///
/// The export must actually exist at `key.file_id`, so a non-exported local
/// symbol (`KeyResolution::LocalOnly` for an unexported const) is never treated
/// as public API and stays reportable.
fn key_is_public_api(
    graph: &ModuleGraph,
    key: &ExportKey,
    public_api_entry_points: &FxHashSet<FileId>,
    entry_star_targets: &FxHashSet<FileId>,
) -> bool {
    let Some(module) = graph.modules.get(key.file_id.0 as usize) else {
        return false;
    };
    let Some(export) = module
        .exports
        .iter()
        .find(|export| export_name_matches(&export.name, &key.export_name))
    else {
        return false;
    };
    public_api_entry_points.contains(&key.file_id)
        || entry_star_targets.contains(&key.file_id)
        || export_has_entry_point_re_export_reference(graph, export, public_api_entry_points)
}

fn export_name_matches(name: &ExportName, target: &str) -> bool {
    match name {
        ExportName::Named(n) => n == target,
        ExportName::Default => target == "default",
    }
}

const fn framework_str(framework: DiFramework) -> &'static str {
    match framework {
        DiFramework::Vue => "vue",
        DiFramework::Svelte => "svelte",
    }
}
