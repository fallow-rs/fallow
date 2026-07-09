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

use super::members::{
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
#[derive(Clone, Copy)]
pub(super) struct UnprovidedInjectInput<'a> {
    pub(super) graph: &'a ModuleGraph,
    pub(super) resolved_modules: &'a [ResolvedModule],
    pub(super) modules: &'a [ModuleInfo],
    pub(super) declared_deps: &'a FxHashSet<String>,
    pub(super) public_api_entry_points: &'a FxHashSet<FileId>,
    pub(super) suppressions: &'a SuppressionContext<'a>,
    pub(super) line_offsets_by_file: &'a LineOffsetsMap<'a>,
}

#[must_use]
pub fn find_unprovided_injects(input: UnprovidedInjectInput<'_>) -> Vec<UnprovidedInject> {
    if !unprovided_inject_active(input) {
        return Vec::new();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        input.modules.iter().map(|m| (m.file_id, m)).collect();
    let path_by_id: FxHashMap<FileId, &std::path::Path> = input
        .graph
        .modules
        .iter()
        .map(|module| (module.file_id, module.path.as_path()))
        .collect();

    let provided = build_provided_key_set(input, &modules_by_id);
    let entry_star_targets =
        entry_point_star_re_export_targets(input.graph, input.public_api_entry_points);

    let scan = InjectScanContext {
        input,
        modules_by_id: &modules_by_id,
        path_by_id: &path_by_id,
        provided: &provided,
        entry_star_targets: &entry_star_targets,
    };
    collect_unprovided_inject_findings(&scan)
}

/// Whether the inject detector runs: a DI dependency is declared and no reachable
/// module has an unknowable-key (dynamic) provide (which would abstain wholesale).
fn unprovided_inject_active(input: UnprovidedInjectInput<'_>) -> bool {
    let vue =
        input.declared_deps.contains("vue") || input.declared_deps.contains("@vue/runtime-core");
    let svelte = input.declared_deps.contains("svelte");
    let angular = input.declared_deps.contains("@angular/core");
    if !vue && !svelte && !angular {
        return false;
    }
    // Dynamic-provide abstain: a single unknowable-key provide anywhere means a
    // surviving inject finding could be a false positive, so abstain wholesale.
    !input
        .modules
        .iter()
        .any(|module| module.has_dynamic_provide)
}

/// Pass 1: build the provided-key set liberally (over-crediting a provided key
/// only suppresses a finding, never creates one).
fn build_provided_key_set(
    input: UnprovidedInjectInput<'_>,
    modules_by_id: &FxHashMap<FileId, &ModuleInfo>,
) -> FxHashSet<ExportKey> {
    let mut provided: FxHashSet<ExportKey> = FxHashSet::default();
    for resolved in input.resolved_modules {
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
            match resolve_key(
                resolved,
                input.graph,
                &local_to_export_keys,
                &site.key_local,
            ) {
                KeyResolution::Internal(keys) | KeyResolution::LocalOnly(keys) => {
                    provided.extend(keys);
                }
                KeyResolution::External => {}
            }
        }
    }
    provided
}

/// Shared read-only state threaded through the inject (Pass 2) scan.
struct InjectScanContext<'a> {
    input: UnprovidedInjectInput<'a>,
    modules_by_id: &'a FxHashMap<FileId, &'a ModuleInfo>,
    path_by_id: &'a FxHashMap<FileId, &'a std::path::Path>,
    provided: &'a FxHashSet<ExportKey>,
    entry_star_targets: &'a FxHashSet<FileId>,
}

/// Pass 2: emit a finding for each inject site whose key is provided nowhere.
fn collect_unprovided_inject_findings(scan: &InjectScanContext<'_>) -> Vec<UnprovidedInject> {
    let mut findings = Vec::new();
    for resolved in scan.input.resolved_modules {
        let Some(module) = scan.modules_by_id.get(&resolved.file_id) else {
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
            if let Some(finding) = evaluate_inject_site(scan, resolved, &local_to_export_keys, site)
            {
                findings.push(finding);
            }
        }
    }
    findings
}

/// Evaluate one inject site against the full abstain ladder (external key,
/// empty canonical, Angular token gate, provided-match, public-API, suppression),
/// returning a finding only when every abstain is cleared.
fn evaluate_inject_site(
    scan: &InjectScanContext<'_>,
    resolved: &ResolvedModule,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    site: &fallow_types::extract::DiKeySite,
) -> Option<UnprovidedInject> {
    if !inject_site_has_unprovided_key(scan, resolved, local_to_export_keys, site) {
        return None;
    }

    let (line, col) = byte_offset_to_line_col(
        scan.input.line_offsets_by_file,
        resolved.file_id,
        site.span_start,
    );
    if inject_site_suppressed(scan, resolved.file_id, line) {
        return None;
    }
    let path = scan.path_by_id.get(&resolved.file_id)?;
    Some(build_unprovided_inject(path, site, line, col))
}

fn inject_site_has_unprovided_key(
    scan: &InjectScanContext<'_>,
    resolved: &ResolvedModule,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    site: &fallow_types::extract::DiKeySite,
) -> bool {
    let canonical = match resolve_key(
        resolved,
        scan.input.graph,
        local_to_export_keys,
        &site.key_local,
    ) {
        // External: the provide may live inside the package; abstain.
        KeyResolution::External => return false,
        KeyResolution::Internal(keys) | KeyResolution::LocalOnly(keys) => keys,
    };
    if canonical.is_empty() {
        return false;
    }
    // Angular InjectionToken FP gate: only a USER `InjectionToken` is in scope. A
    // class / framework token (`inject(MyService)`) is FP-prone via
    // `providedIn: 'root'` and third-party `provideX()`, so abstain unless at
    // least one canonical key is a known InjectionToken (its defining module
    // lists the export name in `injection_tokens`). Vue / Svelte sites skip it.
    if site.framework == DiFramework::Angular
        && !canonical
            .iter()
            .any(|key| is_known_injection_token(scan.modules_by_id, key))
    {
        return false;
    }
    // Matched by a provide somewhere in the project.
    if canonical.iter().any(|key| scan.provided.contains(key)) {
        return false;
    }
    // Public-API abstain: the consumer of this package provides the key.
    if canonical.iter().any(|key| {
        key_is_public_api(
            scan.input.graph,
            key,
            scan.input.public_api_entry_points,
            scan.entry_star_targets,
        )
    }) {
        return false;
    }

    true
}

fn inject_site_suppressed(scan: &InjectScanContext<'_>, file_id: FileId, line: u32) -> bool {
    scan.input
        .suppressions
        .is_suppressed(file_id, line, IssueKind::UnprovidedInject)
        || scan
            .input
            .suppressions
            .is_file_suppressed(file_id, IssueKind::UnprovidedInject)
}

fn build_unprovided_inject(
    path: &std::path::Path,
    site: &fallow_types::extract::DiKeySite,
    line: u32,
    col: u32,
) -> UnprovidedInject {
    UnprovidedInject {
        path: path.to_path_buf(),
        key_name: site.key_local.clone(),
        framework: framework_str(site.framework).to_string(),
        line,
        col,
    }
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

/// Whether a canonical export key names a known Angular `InjectionToken`: the
/// key's defining module lists the export name (`.0`) in `injection_tokens`. This
/// is the load-bearing FP gate that keeps class / framework tokens out of scope.
fn is_known_injection_token(
    modules_by_id: &FxHashMap<FileId, &ModuleInfo>,
    key: &ExportKey,
) -> bool {
    modules_by_id.get(&key.file_id).is_some_and(|module| {
        module
            .injection_tokens
            .iter()
            .any(|(token_name, _interface)| *token_name == key.export_name)
    })
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
        DiFramework::Angular => "angular",
    }
}
