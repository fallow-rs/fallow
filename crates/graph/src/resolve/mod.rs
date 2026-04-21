//! Import specifier resolution using `oxc_resolver`.
//!
//! Orchestrates the resolution pipeline: for every extracted module, resolves all
//! import specifiers in parallel (via rayon) to an [`ResolveResult`] — internal file,
//! npm package, external file, or unresolvable. The entry point is [`resolve_all_imports`].
//!
//! Resolution is split into submodules by import kind:
//! - `static_imports` — ES `import` declarations
//! - `dynamic_imports` — `import()` expressions and glob-based dynamic patterns
//! - `require_imports` — CommonJS `require()` calls
//! - `re_exports` — `export { x } from './y'` re-export sources
//! - `upgrades` — post-resolution pass fixing non-deterministic bare specifier results
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

pub use path_info::{extract_package_name, is_bare_specifier, is_path_alias};
pub use types::{ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport};

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::extract::ModuleInfo;

use dynamic_imports::{resolve_dynamic_imports, resolve_dynamic_patterns};
use re_exports::resolve_re_exports;
use require_imports::resolve_require_imports;
use specifier::create_resolver;
use static_imports::resolve_static_imports;
use types::ResolveContext;
use upgrades::apply_specifier_upgrades;

/// Resolve all imports across all modules in parallel.
#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "resolver inputs come from disjoint sources (config, plugins, workspace, filesystem); \
              bundling them into a struct would be a cross-cutting refactor outside this task"
)]
pub fn resolve_all_imports(
    modules: &[ModuleInfo],
    files: &[DiscoveredFile],
    workspaces: &[fallow_config::WorkspaceInfo],
    active_plugins: &[String],
    path_aliases: &[(String, String)],
    scss_include_paths: &[PathBuf],
    root: &Path,
    extra_conditions: &[String],
) -> Vec<ResolvedModule> {
    // Build workspace name → root index for pnpm store fallback.
    // Canonicalize roots to match path_to_id (which uses canonical paths).
    // Without this, macOS /var → /private/var and similar platform symlinks
    // cause workspace roots to mismatch canonical file paths.
    let canonical_ws_roots: Vec<PathBuf> = workspaces
        .par_iter()
        .map(|ws| dunce::canonicalize(&ws.root).unwrap_or_else(|_| ws.root.clone()))
        .collect();
    let workspace_roots: FxHashMap<&str, &Path> = workspaces
        .iter()
        .zip(canonical_ws_roots.iter())
        .map(|(ws, canonical)| (ws.name.as_str(), canonical.as_path()))
        .collect();

    // Check if project root is already canonical (no symlinks in path).
    // When true, raw paths == canonical paths for files under root, so we can skip
    // the upfront bulk canonicalize() of all source files (21k+ syscalls on large projects).
    // A lazy CanonicalFallback handles the rare intra-project symlink case.
    let root_is_canonical = dunce::canonicalize(root).is_ok_and(|c| c == root);

    // Pre-compute canonical paths ONCE for all files in parallel (avoiding repeated syscalls).
    // Skipped when root is canonical — the lazy fallback below handles edge cases.
    let canonical_paths: Vec<PathBuf> = if root_is_canonical {
        Vec::new()
    } else {
        files
            .par_iter()
            .map(|f| dunce::canonicalize(&f.path).unwrap_or_else(|_| f.path.clone()))
            .collect()
    };

    // Primary path → FileId index. When root is canonical, uses raw paths (fast).
    // Otherwise uses pre-computed canonical paths (correct for all symlink configurations).
    let path_to_id: FxHashMap<&Path, FileId> = if root_is_canonical {
        files.iter().map(|f| (f.path.as_path(), f.id)).collect()
    } else {
        canonical_paths
            .iter()
            .enumerate()
            .map(|(idx, canonical)| (canonical.as_path(), files[idx].id))
            .collect()
    };

    // Also index by non-canonical path for fallback lookups
    let raw_path_to_id: FxHashMap<&Path, FileId> =
        files.iter().map(|f| (f.path.as_path(), f.id)).collect();

    // FileIds are sequential 0..n, so direct array indexing is faster than FxHashMap.
    let file_paths: Vec<&Path> = files.iter().map(|f| f.path.as_path()).collect();

    // Create resolver ONCE and share across threads (oxc_resolver::Resolver is Send + Sync)
    let resolver = create_resolver(active_plugins, extra_conditions);

    // Lazy canonical fallback — only needed when root is canonical (path_to_id uses raw paths).
    // When root is NOT canonical, path_to_id already uses canonical paths, no fallback needed.
    let canonical_fallback = if root_is_canonical {
        Some(types::CanonicalFallback::new(files))
    } else {
        None
    };

    // Dedup set for broken-tsconfig warnings. See `ResolveContext::tsconfig_warned`.
    let tsconfig_warned: Mutex<FxHashSet<String>> = Mutex::new(FxHashSet::default());

    // Shared resolution context — avoids passing 6 arguments to every resolve_specifier call
    let ctx = ResolveContext {
        resolver: &resolver,
        path_to_id: &path_to_id,
        raw_path_to_id: &raw_path_to_id,
        workspace_roots: &workspace_roots,
        path_aliases,
        scss_include_paths,
        root,
        canonical_fallback: canonical_fallback.as_ref(),
        tsconfig_warned: &tsconfig_warned,
    };

    // Resolve in parallel — shared resolver instance.
    // Each file resolves its own imports independently (no shared bare specifier cache).
    // oxc_resolver's internal caches (package.json, tsconfig, directory entries) are
    // shared across threads for performance.
    let mut resolved: Vec<ResolvedModule> = modules
        .par_iter()
        .filter_map(|module| {
            let Some(file_path) = file_paths.get(module.file_id.0 as usize) else {
                tracing::warn!(
                    file_id = module.file_id.0,
                    "Skipping module with unknown file_id during resolution"
                );
                return None;
            };

            let mut all_imports = resolve_static_imports(&ctx, file_path, &module.imports);
            all_imports.extend(resolve_require_imports(
                &ctx,
                file_path,
                &module.require_calls,
            ));

            let from_dir = if canonical_paths.is_empty() {
                // Root is canonical — raw paths are canonical
                file_path.parent().unwrap_or(file_path)
            } else {
                canonical_paths
                    .get(module.file_id.0 as usize)
                    .and_then(|p| p.parent())
                    .unwrap_or(file_path)
            };

            Some(ResolvedModule {
                file_id: module.file_id,
                path: file_path.to_path_buf(),
                exports: module.exports.clone(),
                re_exports: resolve_re_exports(&ctx, file_path, &module.re_exports),
                resolved_imports: all_imports,
                resolved_dynamic_imports: resolve_dynamic_imports(
                    &ctx,
                    file_path,
                    &module.dynamic_imports,
                ),
                resolved_dynamic_patterns: resolve_dynamic_patterns(
                    from_dir,
                    &module.dynamic_import_patterns,
                    &canonical_paths,
                    files,
                ),
                member_accesses: module.member_accesses.clone(),
                whole_object_uses: module.whole_object_uses.clone(),
                has_cjs_exports: module.has_cjs_exports,
                unused_import_bindings: module.unused_import_bindings.iter().cloned().collect(),
            })
        })
        .collect();

    apply_specifier_upgrades(&mut resolved);

    resolved
}
