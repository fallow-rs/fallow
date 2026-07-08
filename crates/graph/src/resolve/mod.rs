//! Import specifier resolution using `oxc_resolver`.
//!
//! Orchestrates the resolution pipeline: for every extracted module, resolves all
//! import specifiers in parallel (via rayon) to an [`ResolveResult`], internal file,
//! npm package, external file, or unresolvable. The entry point is [`resolve_all_imports`].
//!
//! Resolution is split into submodules by import kind:
//! - `static_imports`: ES `import` declarations
//! - `dynamic_imports`: `import()` expressions and glob-based dynamic patterns
//! - `require_imports`: CommonJS `require()` calls
//! - `re_exports`: `export { x } from './y'` re-export sources
//! - `upgrades`: post-resolution pass fixing non-deterministic bare specifier results
//!
//! Handles tsconfig path aliases (auto-discovered per file), pnpm virtual store paths,
//! React Native platform extensions, and package.json `exports` subpath resolution with
//! output-to-source directory fallback.

mod dynamic_imports;
pub(crate) mod fallbacks;
mod path_info;
mod re_exports;
mod react_native;
mod require_imports;
mod specifier;
mod static_imports;
#[cfg(test)]
mod tests;
mod types;
mod upgrades;

pub use fallbacks::extract_package_name_from_node_modules_path;
pub use path_info::{
    extract_package_name, is_bare_specifier, is_path_alias, is_valid_package_name,
};
pub use types::{
    ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport, ResolvedSourceEdge,
};

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::{AutoImportKind, AutoImportRule};
use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::extract::{ImportInfo, ImportedName, ModuleInfo};
use oxc_span::Span;

use dynamic_imports::{resolve_dynamic_imports, resolve_dynamic_patterns};
use re_exports::resolve_re_exports;
use react_native::{build_condition_names, build_extensions};
use require_imports::resolve_require_imports;
use specifier::create_resolver;
use static_imports::resolve_static_imports;
use types::{PackageManifestInfo, ResolveContext};
use upgrades::apply_specifier_upgrades;

/// Inputs used to resolve imports for a complete extracted project.
pub struct ResolveAllImportsInput<'a> {
    /// Extracted modules whose imports should be resolved.
    pub modules: &'a [ModuleInfo],
    /// Discovered source files indexed by [`FileId`].
    pub files: &'a [DiscoveredFile],
    /// Workspace package roots used for package self-resolution.
    pub workspaces: &'a [fallow_config::WorkspaceInfo],
    /// Active plugin names that affect extensions and resolver conditions.
    pub active_plugins: &'a [String],
    /// Configured TypeScript path alias pairs.
    pub path_aliases: &'a [(String, String)],
    /// Auto-import rules that synthesize implicit graph edges.
    pub auto_imports: &'a [AutoImportRule],
    /// Additional Sass and SCSS include directories.
    pub scss_include_paths: &'a [PathBuf],
    /// Static directory mappings for framework-specific asset resolution.
    pub static_dir_mappings: &'a [(PathBuf, String)],
    /// Project root used for package manifest and relative-path resolution.
    pub root: &'a Path,
    /// Extra resolver conditions supplied by configuration.
    pub extra_conditions: &'a [String],
}

/// Reusable per-project resolver state: the resolver instances, package
/// manifests, and workspace canonicalization that do not depend on the specific
/// modules being resolved.
///
/// Building these is the bulk of `resolve_all_imports`'s non-parallel setup cost
/// (workspace `dunce::canonicalize`, root + workspace `package.json` loads, and
/// resolver construction). A caller that resolves many small inputs against the
/// same project (the external-stylesheet scanner resolves dozens of node_modules
/// stylesheets one file at a time) builds a session once and reuses it across
/// every resolution instead of rebuilding this state per call.
pub struct ResolverSession {
    resolver: oxc_resolver::Resolver,
    style_resolver: oxc_resolver::Resolver,
    extensions: Vec<String>,
    condition_names: Vec<String>,
    package_manifests: Vec<PackageManifestInfo>,
    canonical_ws_roots: Vec<PathBuf>,
    root_is_canonical: bool,
}

impl ResolverSession {
    /// Build the reusable resolver state for `input`'s project context (root,
    /// workspaces, active plugins, and resolver conditions).
    ///
    /// The session is valid for any later [`resolve_all_imports_with_session`]
    /// call whose input shares that project context; in particular the
    /// `workspaces` slice must be the same (same entries and order) so workspace
    /// names line up with the cached `canonical_ws_roots`.
    #[must_use]
    pub fn new(input: &ResolveAllImportsInput<'_>) -> Self {
        let canonical_ws_roots: Vec<PathBuf> = input
            .workspaces
            .par_iter()
            .map(|ws| dunce::canonicalize(&ws.root).unwrap_or_else(|_| ws.root.clone()))
            .collect();
        let package_manifests = build_package_manifests(input, &canonical_ws_roots);
        let root_is_canonical = dunce::canonicalize(input.root).is_ok_and(|c| c == input.root);

        let extensions = build_extensions(input.active_plugins);
        let condition_names = build_condition_names(input.active_plugins, input.extra_conditions);
        let resolver = create_resolver(input.active_plugins, input.extra_conditions);
        let mut style_conditions = input.extra_conditions.to_vec();
        style_conditions.push("sass".to_string());
        style_conditions.push("style".to_string());
        let style_resolver = create_resolver(input.active_plugins, &style_conditions);

        Self {
            resolver,
            style_resolver,
            extensions,
            condition_names,
            package_manifests,
            canonical_ws_roots,
            root_is_canonical,
        }
    }
}

/// Resolve all imports across all modules in parallel.
#[must_use]
pub fn resolve_all_imports(input: &ResolveAllImportsInput<'_>) -> Vec<ResolvedModule> {
    let session = ResolverSession::new(input);
    resolve_all_imports_with_session(input, &session)
}

/// Resolve all imports for `input`, reusing a prebuilt [`ResolverSession`].
///
/// This is the single resolution code path; [`resolve_all_imports`] is the
/// convenience wrapper that builds a fresh session first. `session` MUST have
/// been built from an input with the same project context as `input` (same
/// `root`, `workspaces` slice and order, `active_plugins`, and
/// `extra_conditions`); only `modules` and `files` may differ.
#[must_use]
pub fn resolve_all_imports_with_session(
    input: &ResolveAllImportsInput<'_>,
    session: &ResolverSession,
) -> Vec<ResolvedModule> {
    let root_is_canonical = session.root_is_canonical;
    let workspace_roots = build_workspace_roots(input.workspaces, &session.canonical_ws_roots);
    let canonical_paths = build_canonical_file_paths(input.files, root_is_canonical);
    let path_to_id = build_path_to_id(input.files, &canonical_paths, root_is_canonical);
    let raw_path_to_id: FxHashMap<&Path, FileId> = input
        .files
        .iter()
        .map(|f| (f.path.as_path(), f.id))
        .collect();

    let file_paths: Vec<&Path> = input.files.iter().map(|f| f.path.as_path()).collect();

    let canonical_fallback = if root_is_canonical {
        Some(types::CanonicalFallback::new(input.files))
    } else {
        None
    };

    let tsconfig_warned: Mutex<FxHashSet<String>> = Mutex::new(FxHashSet::default());
    let tsconfig_cache = types::TsconfigCache::default();
    let canonicalize_cache = types::CanonicalizeCache::default();

    let ctx = ResolveContext {
        resolver: &session.resolver,
        style_resolver: &session.style_resolver,
        extensions: &session.extensions,
        path_to_id: &path_to_id,
        raw_path_to_id: &raw_path_to_id,
        workspace_roots: &workspace_roots,
        package_manifests: &session.package_manifests,
        condition_names: &session.condition_names,
        path_aliases: input.path_aliases,
        scss_include_paths: input.scss_include_paths,
        static_dir_mappings: input.static_dir_mappings,
        root: input.root,
        canonical_fallback: canonical_fallback.as_ref(),
        tsconfig_warned: &tsconfig_warned,
        tsconfig_cache: &tsconfig_cache,
        canonicalize_cache: &canonicalize_cache,
    };

    let mut resolved: Vec<ResolvedModule> = input
        .modules
        .par_iter()
        .filter_map(|module| {
            resolve_module_imports(module, &ctx, &file_paths, &canonical_paths, input.files)
        })
        .collect();

    apply_specifier_upgrades(&mut resolved);

    synthesize_auto_import_edges(
        &mut resolved,
        input.modules,
        input.auto_imports,
        &path_to_id,
        &raw_path_to_id,
    );

    resolved
}

fn build_workspace_roots<'a>(
    workspaces: &'a [fallow_config::WorkspaceInfo],
    canonical_ws_roots: &'a [PathBuf],
) -> FxHashMap<&'a str, &'a Path> {
    workspaces
        .iter()
        .zip(canonical_ws_roots.iter())
        .map(|(ws, canonical)| (ws.name.as_str(), canonical.as_path()))
        .collect()
}

fn build_canonical_file_paths(files: &[DiscoveredFile], root_is_canonical: bool) -> Vec<PathBuf> {
    if root_is_canonical {
        return Vec::new();
    }

    files
        .par_iter()
        .map(|f| dunce::canonicalize(&f.path).unwrap_or_else(|_| f.path.clone()))
        .collect()
}

/// Load the root package manifest plus each workspace manifest into the
/// `PackageManifestInfo` list used for `exports` / `imports` resolution.
fn build_package_manifests(
    input: &ResolveAllImportsInput<'_>,
    canonical_ws_roots: &[PathBuf],
) -> Vec<PackageManifestInfo> {
    let root_canonical =
        dunce::canonicalize(input.root).unwrap_or_else(|_| input.root.to_path_buf());
    let mut package_manifests = Vec::new();
    if let Ok(package_json) = fallow_config::PackageJson::load(&input.root.join("package.json")) {
        package_manifests.push(PackageManifestInfo {
            root: input.root.to_path_buf(),
            canonical_root: root_canonical,
            name: package_json.name.clone(),
            package_json,
        });
    }
    for (ws, canonical_root) in input.workspaces.iter().zip(canonical_ws_roots.iter()) {
        if let Ok(package_json) = fallow_config::PackageJson::load(&ws.root.join("package.json")) {
            package_manifests.push(PackageManifestInfo {
                root: ws.root.clone(),
                canonical_root: canonical_root.clone(),
                name: package_json.name.clone().or_else(|| Some(ws.name.clone())),
                package_json,
            });
        }
    }
    package_manifests
}

/// Build the path-to-`FileId` index, keyed by canonical paths when the root is
/// not already canonical and by raw paths otherwise.
fn build_path_to_id<'a>(
    files: &'a [DiscoveredFile],
    canonical_paths: &'a [PathBuf],
    root_is_canonical: bool,
) -> FxHashMap<&'a Path, FileId> {
    if root_is_canonical {
        files.iter().map(|f| (f.path.as_path(), f.id)).collect()
    } else {
        canonical_paths
            .iter()
            .enumerate()
            .map(|(idx, canonical)| (canonical.as_path(), files[idx].id))
            .collect()
    }
}

fn resolve_module_imports(
    module: &ModuleInfo,
    ctx: &ResolveContext<'_>,
    file_paths: &[&Path],
    canonical_paths: &[PathBuf],
    files: &[DiscoveredFile],
) -> Option<ResolvedModule> {
    let Some(file_path) = file_paths.get(module.file_id.0 as usize) else {
        tracing::warn!(
            file_id = module.file_id.0,
            "Skipping module with unknown file_id during resolution"
        );
        return None;
    };

    let mut all_imports = resolve_static_imports(ctx, file_path, &module.imports);
    all_imports.extend(resolve_require_imports(
        ctx,
        file_path,
        &module.require_calls,
    ));

    let from_dir = if canonical_paths.is_empty() {
        file_path.parent().unwrap_or(file_path)
    } else {
        canonical_paths
            .get(module.file_id.0 as usize)
            .and_then(|p| p.parent())
            .unwrap_or(file_path)
    };

    Some(build_resolved_module(ResolvedModuleBuildInput {
        module,
        ctx,
        file_path,
        from_dir,
        canonical_paths,
        files,
        all_imports,
    }))
}

struct ResolvedModuleBuildInput<'a> {
    module: &'a ModuleInfo,
    ctx: &'a ResolveContext<'a>,
    file_path: &'a Path,
    from_dir: &'a Path,
    canonical_paths: &'a [PathBuf],
    files: &'a [DiscoveredFile],
    all_imports: Vec<types::ResolvedImport>,
}

fn build_resolved_module(input: ResolvedModuleBuildInput<'_>) -> ResolvedModule {
    ResolvedModule {
        file_id: input.module.file_id,
        path: input.file_path.to_path_buf(),
        exports: input.module.exports.clone(),
        re_exports: resolve_re_exports(input.ctx, input.file_path, &input.module.re_exports),
        resolved_imports: input.all_imports,
        resolved_dynamic_imports: resolve_dynamic_imports(
            input.ctx,
            input.file_path,
            &input.module.dynamic_imports,
        ),
        resolved_dynamic_patterns: resolve_dynamic_patterns(
            input.from_dir,
            &input.module.dynamic_import_patterns,
            input.canonical_paths,
            input.files,
        ),
        member_accesses: input.module.member_accesses.clone(),
        semantic_facts: input.module.semantic_facts.clone(),
        whole_object_uses: input.module.whole_object_uses.clone(),
        has_cjs_exports: input.module.has_cjs_exports,
        has_angular_component_template_url: input.module.has_angular_component_template_url,
        unused_import_bindings: input
            .module
            .unused_import_bindings
            .iter()
            .cloned()
            .collect(),
        type_referenced_import_bindings: input.module.type_referenced_import_bindings.clone(),
        value_referenced_import_bindings: input.module.value_referenced_import_bindings.clone(),
        namespace_object_aliases: input.module.namespace_object_aliases.clone(),
        exported_factory_returns: input.module.exported_factory_returns.clone(),
        type_member_types: input.module.type_member_types.clone(),
    }
}

/// Synthesize module-graph edges for convention auto-imports.
///
/// For each module, every captured `auto_import_candidates` name is matched
/// against the active plugins' auto-import table; on a hit a synthetic
/// [`ResolvedImport`] is added so the existing graph builder credits the edge.
/// Name collisions across files over-credit every match, keeping each provider
/// reachable. Resolution is recomputed from the live file index each run.
fn synthesize_auto_import_edges(
    resolved: &mut [ResolvedModule],
    modules: &[ModuleInfo],
    auto_imports: &[AutoImportRule],
    path_to_id: &FxHashMap<&Path, FileId>,
    raw_path_to_id: &FxHashMap<&Path, FileId>,
) {
    if auto_imports.is_empty() {
        return;
    }

    let mut table: FxHashMap<&str, Vec<(FileId, AutoImportKind)>> = FxHashMap::default();
    for rule in auto_imports {
        let source = rule.source.as_path();
        let Some(file_id) = raw_path_to_id
            .get(source)
            .or_else(|| path_to_id.get(source))
            .copied()
        else {
            continue;
        };
        table
            .entry(rule.name.as_str())
            .or_default()
            .push((file_id, rule.kind));
    }
    if table.is_empty() {
        return;
    }

    let candidates: FxHashMap<FileId, &[String]> = modules
        .iter()
        .filter(|module| !module.auto_import_candidates.is_empty())
        .map(|module| (module.file_id, module.auto_import_candidates.as_slice()))
        .collect();
    if candidates.is_empty() {
        return;
    }

    for module in resolved.iter_mut() {
        let Some(names) = candidates.get(&module.file_id) else {
            continue;
        };
        for name in *names {
            if is_auto_import_builtin(name) {
                continue;
            }
            let Some(targets) = table.get(name.as_str()) else {
                continue;
            };
            for (target_id, kind) in targets {
                if *target_id == module.file_id {
                    continue;
                }
                module.resolved_imports.push(ResolvedImport {
                    info: synthetic_auto_import_info(name, *kind),
                    target: ResolveResult::SyntheticAutoImport(*target_id),
                });
            }
        }
    }
}

fn is_auto_import_builtin(name: &str) -> bool {
    is_js_auto_import_builtin(name)
        || is_vue_auto_import_builtin(name)
        || is_nuxt_auto_import_builtin(name)
}

fn is_js_auto_import_builtin(name: &str) -> bool {
    matches!(
        name,
        "AbortController"
            | "AbortSignal"
            | "Array"
            | "ArrayBuffer"
            | "BigInt"
            | "Blob"
            | "Boolean"
            | "Buffer"
            | "CSS"
            | "DOMParser"
            | "Date"
            | "Document"
            | "Error"
            | "Event"
            | "EventTarget"
            | "File"
            | "FormData"
            | "Intl"
            | "JSON"
            | "Map"
            | "Math"
            | "Number"
            | "Object"
            | "Promise"
            | "Reflect"
            | "RegExp"
            | "Response"
            | "Set"
            | "String"
            | "Symbol"
            | "URL"
            | "URLSearchParams"
            | "WeakMap"
            | "WeakSet"
            | "Window"
            | "alert"
            | "clearInterval"
            | "clearTimeout"
            | "console"
            | "document"
            | "fetch"
            | "global"
            | "globalThis"
            | "localStorage"
            | "navigator"
            | "process"
            | "requestAnimationFrame"
            | "sessionStorage"
            | "setInterval"
            | "setTimeout"
            | "window"
    )
}

fn is_vue_auto_import_builtin(name: &str) -> bool {
    matches!(name, |"computed"| "customRef"
        | "defineAsyncComponent"
        | "defineComponent"
        | "effectScope"
        | "getCurrentInstance"
        | "h"
        | "inject"
        | "isProxy"
        | "isReactive"
        | "isReadonly"
        | "isRef"
        | "markRaw"
        | "nextTick"
        | "onActivated"
        | "onBeforeMount"
        | "onBeforeUnmount"
        | "onBeforeUpdate"
        | "onDeactivated"
        | "onErrorCaptured"
        | "onMounted"
        | "onRenderTracked"
        | "onRenderTriggered"
        | "onScopeDispose"
        | "onServerPrefetch"
        | "onUnmounted"
        | "onUpdated"
        | "provide"
        | "reactive"
        | "readonly"
        | "ref"
        | "resolveComponent"
        | "shallowReactive"
        | "shallowReadonly"
        | "shallowRef"
        | "toRaw"
        | "toRef"
        | "toRefs"
        | "triggerRef"
        | "unref"
        | "watch"
        | "watchEffect"
        | "watchPostEffect"
        | "watchSyncEffect")
}

fn is_nuxt_auto_import_builtin(name: &str) -> bool {
    matches!(name, |"useAsyncData"| "useCookie"
        | "useError"
        | "useFetch"
        | "useHead"
        | "useLazyAsyncData"
        | "useLazyFetch"
        | "useNuxtApp"
        | "useRequestEvent"
        | "useRequestHeaders"
        | "useRoute"
        | "useRouter"
        | "useRuntimeConfig"
        | "useSeoMeta"
        | "useState")
}

/// Build a synthetic [`ImportInfo`] for a convention auto-import. Component and
/// default kinds credit the default export; named kinds credit the named export.
fn synthetic_auto_import_info(name: &str, kind: AutoImportKind) -> ImportInfo {
    let imported_name = match kind {
        AutoImportKind::Named => ImportedName::Named(name.to_string()),
        AutoImportKind::Default | AutoImportKind::DefaultComponent => ImportedName::Default,
    };
    ImportInfo {
        source: format!("<auto-import:{name}>"),
        imported_name,
        local_name: name.to_string(),
        is_type_only: false,
        from_style: false,
        span: Span::default(),
        source_span: Span::default(),
    }
}
