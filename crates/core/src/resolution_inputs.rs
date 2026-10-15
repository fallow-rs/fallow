//! Hash of the non-source files and workspace facts that import resolution
//! reads, for the graph-cache mode.
//!
//! The graph-cache manifest fingerprints discovered source files only. The
//! resolver also reads `package.json` (`imports`, `exports`, `name`), Deno
//! configs (`name`, `exports`, import maps) and every `tsconfig.json` that
//! `oxc_resolver` discovers next to a source file, plus its `extends` and
//! `references` chain. An edit to one of these files changes import targets
//! without a change to any source file, so the mode must cover them.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use fallow_config::WorkspaceInfo;
use fallow_types::discover::DiscoveredFile;
use rustc_hash::{FxHashSet, FxHasher};

/// Manifests that the resolver loads from the project root and from each
/// workspace root (package maps, Deno names, exports and import maps).
const PACKAGE_MANIFESTS: &[&str] = &["package.json", "deno.json", "deno.jsonc"];

/// Files that `oxc_resolver` looks up in every ancestor directory of an
/// importing file: the nearest `package.json` (for `imports`) and the nearest
/// owning `tsconfig.json` (automatic tsconfig discovery).
const NEAREST_CONFIGS: &[&str] = &["package.json", "tsconfig.json"];

/// Hash the workspace `(name, root)` pairs and the content of every resolver
/// input file. Every path is spelled relative to `root`, so a copy of the
/// project under another absolute path hashes the same.
pub fn resolution_inputs_hash(
    root: &Path,
    workspaces: &[WorkspaceInfo],
    files: &[DiscoveredFile],
) -> u64 {
    let mut hasher = FxHasher::default();

    let mut workspace_keys: Vec<(&str, String)> = workspaces
        .iter()
        .map(|ws| (ws.name.as_str(), portable_key(root, &ws.root)))
        .collect();
    workspace_keys.sort_unstable();
    workspace_keys.len().hash(&mut hasher);
    for key in &workspace_keys {
        key.hash(&mut hasher);
    }

    let mut inputs: Vec<(String, Vec<u8>)> = collect_input_files(root, workspaces, files)
        .into_iter()
        .filter_map(|path| {
            let content = std::fs::read(&path).ok()?;
            Some((portable_key(root, &path), content))
        })
        .collect();
    inputs.sort_unstable();
    inputs.len().hash(&mut hasher);
    for (key, content) in &inputs {
        key.hash(&mut hasher);
        content.hash(&mut hasher);
    }

    hasher.finish()
}

fn collect_input_files(
    root: &Path,
    workspaces: &[WorkspaceInfo],
    files: &[DiscoveredFile],
) -> FxHashSet<PathBuf> {
    let mut inputs = FxHashSet::default();
    let manifest_dirs = std::iter::once(root).chain(workspaces.iter().map(|ws| ws.root.as_path()));
    for dir in manifest_dirs {
        for name in PACKAGE_MANIFESTS {
            insert_if_file(&mut inputs, dir.join(name));
        }
        // The output-path fallback reads every root-level `tsconfig*.json`.
        for config in root_level_tsconfigs(dir) {
            inputs.insert(config);
        }
    }

    for dir in source_dirs_and_ancestors(root, files) {
        for name in NEAREST_CONFIGS {
            insert_if_file(&mut inputs, dir.join(name));
        }
    }

    let tsconfigs: Vec<PathBuf> = inputs
        .iter()
        .filter(|path| is_tsconfig_name(path))
        .cloned()
        .collect();
    for config in tsconfigs {
        inputs.extend(fallow_config::tsconfig_linked_files(&config));
    }
    inputs
}

fn insert_if_file(inputs: &mut FxHashSet<PathBuf>, path: PathBuf) {
    if path.is_file() {
        inputs.insert(path);
    }
}

/// Every directory that holds a source file, each ancestor of it up to the
/// project root, and each ancestor of the project root itself. Automatic
/// tsconfig discovery and nearest-`package.json` lookup walk the same chain.
fn source_dirs_and_ancestors(root: &Path, files: &[DiscoveredFile]) -> FxHashSet<PathBuf> {
    let mut dirs: FxHashSet<PathBuf> = root.ancestors().map(Path::to_path_buf).collect();
    for file in files {
        let mut dir = file.path.parent();
        while let Some(current) = dir {
            if !current.starts_with(root) || !dirs.insert(current.to_path_buf()) {
                break;
            }
            dir = current.parent();
        }
    }
    dirs
}

fn root_level_tsconfigs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_tsconfig_name(path) && path.is_file())
        .collect()
}

fn is_tsconfig_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("tsconfig"))
        && path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("json") || extension.eq_ignore_ascii_case("jsonc")
        })
}

/// Spell `path` relative to `root`, with `../` steps for a path outside the
/// root (an ancestor `tsconfig.json`, an `extends` target in a parent repo).
fn portable_key(root: &Path, path: &Path) -> String {
    for (depth, base) in root.ancestors().enumerate() {
        if let Ok(rest) = path.strip_prefix(base) {
            let mut key = "../".repeat(depth);
            key.push_str(&rest.to_string_lossy().replace('\\', "/"));
            return key;
        }
    }
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_key_spells_inside_and_outside_paths_relative_to_root() {
        let root = Path::new("/repo/app");
        assert_eq!(
            portable_key(root, Path::new("/repo/app/src/a.json")),
            "src/a.json"
        );
        assert_eq!(
            portable_key(root, Path::new("/repo/tsconfig.json")),
            "../tsconfig.json"
        );
        assert_eq!(portable_key(root, Path::new("/repo/app")), "");
    }

    #[test]
    fn tsconfig_names_match_root_level_variants_only() {
        assert!(is_tsconfig_name(Path::new("tsconfig.json")));
        assert!(is_tsconfig_name(Path::new("tsconfig.app.jsonc")));
        assert!(!is_tsconfig_name(Path::new("package.json")));
        assert!(!is_tsconfig_name(Path::new("tsconfig.ts")));
    }
}
