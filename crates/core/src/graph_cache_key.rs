//! The parts of the graph cache key that come from the run options, the
//! entry points and the plugin results. `build_graph_cache_manifest` in the
//! crate root combines them with the source fingerprints.

use rustc_hash::FxHashSet;

use crate::{discover, plugins, resolution_inputs, resolve};
use fallow_config::ResolvedConfig;

/// The resolver input that the plugin results give, with no modules, files
/// or workspaces. Import resolution and [`plugin_config_hash`] both read the
/// plugin results through this function. The external stylesheet scanner
/// builds its own input from a subset of the same fields.
pub fn plugin_resolve_input<'a>(
    plugin_result: &'a plugins::AggregatedPluginResult,
    root: &'a std::path::Path,
    extra_conditions: &'a [String],
) -> resolve::ResolveAllImportsInput<'a> {
    resolve::ResolveAllImportsInput {
        modules: &[],
        files: &[],
        workspaces: &[],
        active_plugins: &plugin_result.active_plugins,
        path_aliases: &plugin_result.path_aliases,
        scoped_path_aliases: &plugin_result.scoped_path_aliases,
        auto_imports: &plugin_result.auto_imports,
        jsx_import_sources: &plugin_result.jsx_import_sources,
        scss_include_paths: &plugin_result.scss_include_paths,
        static_dir_mappings: &plugin_result.static_dir_mappings,
        framework_static_dir_mappings: &plugin_result.framework_static_dir_mappings,
        root,
        extra_conditions,
    }
}

/// Hash the resolver-affecting options: the extraction config hash, the
/// user-supplied resolve `conditions`, and the resolver input files and
/// workspace names (see [`resolution_inputs::resolution_inputs_hash`]).
///
/// The source manifest fingerprints source files only. A `package.json`
/// `imports` map, a workspace rename, a Deno import map or a discovered
/// `tsconfig.json` can move an import target without a source edit, so their
/// content keys the mode too. A config above the root is keyed with `../`
/// steps, so its path does not bring the absolute root back into the hash.
///
/// The project root is deliberately NOT hashed. It is not a resolver option:
/// it is where the project happens to sit, and hashing it meant a container
/// job, a matrix over roots, or a copied worktree could never reuse a cache it
/// had fully decoded. Every path this mode covers is hashed root-relative
/// instead.
///
/// `cache_config_hash` does NOT fold tsconfig `paths`, despite what this
/// comment used to claim: it hashes extraction-affecting config plus external
/// plugin names. tsconfig-derived aliases reach the mode through
/// [`hash_path_aliases`].
///
/// `production` and `ignore_patterns` intentionally stay out of this hash:
/// they shape discovery, so changed file sets already miss through stable file
/// keys and source fingerprints in the manifest.
pub fn resolver_options_hash(
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    files: &[discover::DiscoveredFile],
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    config.cache_config_hash.hash(&mut hasher);
    config.resolve.conditions.hash(&mut hasher);
    resolution_inputs::resolution_inputs_hash(&config.root, workspaces, files).hash(&mut hasher);
    hasher.finish()
}

/// Render `path` the way every graph-cache mode hash spells a path: relative to
/// the project root where possible, with forward slashes, so an identical
/// project under a different absolute path hashes identically.
fn root_relative_key(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Hash the entry-point set (sorted root-relative paths per role) so any change
/// in reachability roots misses the cache. The declaration files the graph
/// does not seed are reachability roots too: they depend on package.json,
/// tsconfig and sibling files the manifest does not track.
pub fn entry_points_hash(
    entry_points: &discover::CategorizedEntryPoints,
    files: &[discover::DiscoveredFile],
    unseeded_declaration_files: &FxHashSet<discover::FileId>,
    root: &std::path::Path,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    for role in [&entry_points.all, &entry_points.runtime, &entry_points.test] {
        let mut keys: Vec<String> = role
            .iter()
            .map(|ep| root_relative_key(root, &ep.path))
            .collect();
        keys.sort_unstable();
        keys.len().hash(&mut hasher);
        for key in keys {
            key.hash(&mut hasher);
        }
    }
    let excluded = entry_points.production_excluded();
    excluded.len().hash(&mut hasher);
    for path in excluded {
        root_relative_key(root, path).hash(&mut hasher);
    }
    let mut unseeded: Vec<String> = files
        .iter()
        .filter(|file| unseeded_declaration_files.contains(&file.id))
        .map(|file| root_relative_key(root, &file.path))
        .collect();
    unseeded.sort_unstable();
    unseeded.len().hash(&mut hasher);
    for key in unseeded {
        key.hash(&mut hasher);
    }
    hasher.finish()
}

/// Hash the plugin-derived graph-affecting configuration, with every path
/// spelled root-relative.
///
/// The hash reads the plugin results through [`plugin_resolve_input`], the
/// same view that import resolution gets. The destructure names each field of
/// `ResolveAllImportsInput`. A new resolver field does not compile until this
/// function hashes it, or ignores it with a comment that names its key.
///
/// The resolver does not read the entry patterns, setup files and always-used
/// files. They reach the graph through the entry points, which
/// [`entry_points_hash`] keys.
pub fn plugin_config_hash(
    plugin_result: &plugins::AggregatedPluginResult,
    root: &std::path::Path,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let resolve::ResolveAllImportsInput {
        // The manifest keys the files and modules. `resolver_options_hash`
        // keys the workspaces and the conditions.
        modules: _,
        files: _,
        workspaces: _,
        extra_conditions: _,
        // Each path below is hashed root-relative, so the root stays out.
        root: _,
        active_plugins,
        path_aliases,
        scoped_path_aliases,
        auto_imports,
        jsx_import_sources,
        scss_include_paths,
        static_dir_mappings,
        framework_static_dir_mappings,
    } = plugin_resolve_input(plugin_result, root, &[]);
    let mut hasher = rustc_hash::FxHasher::default();

    hash_active_plugins(active_plugins, &mut hasher);
    hash_path_aliases(path_aliases, root, &mut hasher);
    hash_scoped_path_aliases(scoped_path_aliases, root, &mut hasher);

    let mut auto_imports: Vec<AutoImportHashKey<'_>> = auto_imports
        .iter()
        .map(|rule| {
            let mut scope: Vec<String> = rule
                .scope
                .iter()
                .map(|scope_root| root_relative_key(root, scope_root))
                .collect();
            scope.sort_unstable();
            (
                rule.name.as_str(),
                root_relative_key(root, &rule.source),
                auto_import_kind_rank(rule.kind),
                scope,
            )
        })
        .collect();
    auto_imports.sort_unstable();
    auto_imports.len().hash(&mut hasher);
    for key in &auto_imports {
        key.hash(&mut hasher);
    }

    fn sorted(patterns: &[String]) -> Vec<&str> {
        let mut patterns: Vec<&str> = patterns.iter().map(String::as_str).collect();
        patterns.sort_unstable();
        patterns
    }
    let mut jsx_import_sources: Vec<(&str, String, Vec<&str>, Vec<&str>)> = jsx_import_sources
        .iter()
        .map(|rule| {
            (
                rule.source.as_str(),
                root_relative_key(root, &rule.config_dir),
                sorted(&rule.include),
                sorted(&rule.exclude),
            )
        })
        .collect();
    jsx_import_sources.sort_unstable();
    jsx_import_sources.dedup();
    jsx_import_sources.len().hash(&mut hasher);
    for key in &jsx_import_sources {
        key.hash(&mut hasher);
    }

    let mut scss_include_paths: Vec<String> = scss_include_paths
        .iter()
        .map(|path| root_relative_key(root, path))
        .collect();
    scss_include_paths.sort_unstable();
    scss_include_paths.len().hash(&mut hasher);
    for path in scss_include_paths {
        path.hash(&mut hasher);
    }

    hash_static_dir_mappings(static_dir_mappings, root, &mut hasher);
    hash_static_dir_mappings(framework_static_dir_mappings, root, &mut hasher);

    hasher.finish()
}

/// Hash `(directory, mount)` pairs sorted, with each directory root-relative.
fn hash_static_dir_mappings(
    mappings: &[(std::path::PathBuf, String)],
    root: &std::path::Path,
    hasher: &mut rustc_hash::FxHasher,
) {
    use std::hash::Hash;
    let mut keys: Vec<(String, &str)> = mappings
        .iter()
        .map(|(from_dir, mount)| (root_relative_key(root, from_dir), mount.as_str()))
        .collect();
    keys.sort_unstable();
    keys.len().hash(hasher);
    for (from_dir, mount) in keys {
        from_dir.hash(hasher);
        mount.hash(hasher);
    }
}

fn hash_active_plugins(active_plugins: &[String], hasher: &mut rustc_hash::FxHasher) {
    use std::hash::Hash;
    let mut active: Vec<&str> = active_plugins.iter().map(String::as_str).collect();
    active.sort_unstable();
    active.len().hash(hasher);
    for name in active {
        name.hash(hasher);
    }
}

/// Hash tsconfig- and plugin-derived path aliases. Replacements are absolute
/// filesystem paths, so they are spelled root-relative like every other path in
/// the mode hash.
fn hash_path_aliases(
    path_aliases: &[(String, String)],
    root: &std::path::Path,
    hasher: &mut rustc_hash::FxHasher,
) {
    use std::hash::Hash;
    let mut aliases: Vec<(&str, String)> = path_aliases
        .iter()
        .map(|(prefix, replacement)| {
            (
                prefix.as_str(),
                root_relative_key(root, std::path::Path::new(replacement)),
            )
        })
        .collect();
    aliases.sort_unstable();
    aliases.len().hash(hasher);
    for (prefix, replacement) in aliases {
        prefix.hash(hasher);
        replacement.hash(hasher);
    }
}

/// Hash the workspace-scoped plugin path aliases with their workspace, so a
/// changed alias or a moved workspace misses the cache.
fn hash_scoped_path_aliases(
    aliases: &[resolve::ScopedPathAlias],
    root: &std::path::Path,
    hasher: &mut rustc_hash::FxHasher,
) {
    use std::hash::Hash;
    let mut keys: Vec<(String, &str, String)> = aliases
        .iter()
        .map(|alias| {
            (
                root_relative_key(root, &root.join(&alias.scope)),
                alias.prefix.as_str(),
                root_relative_key(root, &root.join(&alias.replacement)),
            )
        })
        .collect();
    keys.sort_unstable();
    keys.len().hash(hasher);
    keys.hash(hasher);
}

/// The fields of one auto-import rule that decide its graph edges: name,
/// root-relative source, kind rank, and sorted root-relative scope.
type AutoImportHashKey<'a> = (&'a str, String, u8, Vec<String>);

fn auto_import_kind_rank(kind: fallow_config::AutoImportKind) -> u8 {
    match kind {
        fallow_config::AutoImportKind::Named => 0,
        fallow_config::AutoImportKind::Default => 1,
        fallow_config::AutoImportKind::DefaultComponent => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_with_scoped_alias(scope: &str, replacement: &str) -> u64 {
        let mut result = plugins::AggregatedPluginResult::default();
        result.scoped_path_aliases.push(resolve::ScopedPathAlias {
            scope: scope.to_string(),
            prefix: "$lib/".to_string(),
            replacement: replacement.to_string(),
        });
        plugin_config_hash(&result, std::path::Path::new("/project"))
    }

    #[test]
    fn scoped_path_alias_changes_the_plugin_hash() {
        let base = hash_with_scoped_alias("apps/alpha", "apps/alpha/src/lib");
        assert_eq!(
            base,
            hash_with_scoped_alias("apps/alpha", "apps/alpha/src/lib")
        );
        assert_ne!(
            base,
            hash_with_scoped_alias("apps/alpha", "apps/alpha/src/library")
        );
        assert_ne!(
            base,
            hash_with_scoped_alias("apps/beta", "apps/alpha/src/lib")
        );
        assert_ne!(
            base,
            plugin_config_hash(
                &plugins::AggregatedPluginResult::default(),
                std::path::Path::new("/project")
            )
        );
    }
}
