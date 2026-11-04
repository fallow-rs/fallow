//! Hash of the non-source files and workspace facts that import resolution
//! reads, for the graph-cache mode.
//!
//! The graph-cache manifest fingerprints discovered source files only. The
//! resolver also reads `package.json` (`imports`, `exports`, `name`), Deno
//! configs (`name`, `exports`, import maps) and every `tsconfig.json` that
//! `oxc_resolver` discovers next to a source file, plus its `extends` and
//! `references` chain. An edit to one of these files changes import targets
//! without a change to any source file, so the mode must cover them.
//!
//! The resolver also reads `node_modules`, so an install changes import
//! targets too. A walk of `node_modules` costs too much, so the hash reads the
//! lockfile and the install marker that the package manager writes on each
//! install.

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

/// Lockfiles and install markers that change when a package manager adds,
/// removes or updates a package in `node_modules`. Each path is relative to
/// the project root or to a workspace root.
///
/// The hash reads the content of each file, not the modification time. A
/// fresh install in CI writes the same content with a new time, so a time
/// would make each restored cache miss. `node_modules/.modules.yaml` of pnpm
/// holds an install timestamp, so the hash reads the copy of the lockfile in
/// `node_modules/.pnpm/lock.yaml` instead.
const INSTALL_STATE_FILES: &[&str] = &[
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "node_modules/.package-lock.json",
    "node_modules/.pnpm/lock.yaml",
    "node_modules/.yarn-state.yml",
    "node_modules/.yarn-integrity",
];

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

    let mut entry_targets = package_entry_target_states(root, workspaces);
    entry_targets.sort_unstable();
    entry_targets.len().hash(&mut hasher);
    for target in &entry_targets {
        target.hash(&mut hasher);
    }

    let mut install_states = node_modules_states(root, workspaces);
    install_states.sort_unstable();
    install_states.len().hash(&mut hasher);
    for state in &install_states {
        state.hash(&mut hasher);
    }

    hasher.finish()
}

/// Return `(path, exists)` for the `node_modules` directory of the project
/// root and of each workspace root.
///
/// The resolver reports a bare import with a Vite asset query as unresolved
/// only when the package is installed, so an install changes the result
/// without an edit to a source file or a manifest. Only the existence keys the
/// mode: a modification time changes on every fresh install in CI and would
/// make each restored cache miss. The lockfiles and install markers in
/// [`INSTALL_STATE_FILES`] key an install into an existing `node_modules`.
fn node_modules_states(root: &Path, workspaces: &[WorkspaceInfo]) -> Vec<(String, bool)> {
    std::iter::once(root)
        .chain(workspaces.iter().map(|ws| ws.root.as_path()))
        .map(|dir| {
            let path = dir.join("node_modules");
            (portable_key(root, &path), path.is_dir())
        })
        .collect()
}

/// `package.json` fields whose paths the resolver follows for a package
/// import. `exports` is read separately because it nests.
const PACKAGE_ENTRY_FIELDS: &[&str] = &["main", "module", "types", "typings"];

/// Return `(path, exists)` for every static entry path that the project and
/// workspace `package.json` files name.
///
/// The resolver follows an entry that exists on disk (often build output that
/// discovery ignores) and maps an entry that is absent back to source. A build
/// that creates or removes the output changes the import target without an
/// edit to a source file or a manifest, so the existence of each entry keys
/// the mode too.
fn package_entry_target_states(root: &Path, workspaces: &[WorkspaceInfo]) -> Vec<(String, bool)> {
    let mut states = Vec::new();
    let package_dirs = std::iter::once(root).chain(workspaces.iter().map(|ws| ws.root.as_path()));
    for dir in package_dirs {
        let Ok(content) = std::fs::read(dir.join("package.json")) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&content) else {
            continue;
        };
        let mut targets = Vec::new();
        for field in PACKAGE_ENTRY_FIELDS {
            if let Some(serde_json::Value::String(target)) = manifest.get(*field) {
                targets.push(target.as_str());
            }
        }
        if let Some(exports) = manifest.get("exports") {
            collect_export_targets(exports, &mut targets);
        }
        for target in targets {
            if target.contains('*') {
                continue;
            }
            let relative = target.strip_prefix("./").unwrap_or(target);
            if relative.is_empty() || relative.starts_with('/') {
                continue;
            }
            let path = dir.join(relative);
            states.push((portable_key(root, &path), path.exists()));
        }
    }
    states
}

fn collect_export_targets<'a>(value: &'a serde_json::Value, targets: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::String(target) => targets.push(target),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_export_targets(item, targets);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values() {
                collect_export_targets(item, targets);
            }
        }
        _ => {}
    }
}

fn collect_input_files(
    root: &Path,
    workspaces: &[WorkspaceInfo],
    files: &[DiscoveredFile],
) -> FxHashSet<PathBuf> {
    let mut inputs = FxHashSet::default();
    let manifest_dirs = std::iter::once(root).chain(workspaces.iter().map(|ws| ws.root.as_path()));
    for dir in manifest_dirs {
        for name in PACKAGE_MANIFESTS.iter().chain(INSTALL_STATE_FILES) {
            insert_if_file(&mut inputs, dir.join(name));
        }
        // The output-path fallback reads every root-level `tsconfig*.json`.
        for config in root_level_tsconfigs(dir) {
            inputs.insert(config);
        }
    }

    // The resolver also reads `node_modules` above the root, for example in a
    // monorepo where the project root is one package of the repository.
    for dir in root.ancestors().skip(1) {
        for name in INSTALL_STATE_FILES {
            insert_if_file(&mut inputs, dir.join(name));
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

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        std::fs::write(path, content).expect("file");
    }

    #[test]
    fn install_markers_of_root_and_workspaces_key_the_hash() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("repo");
        let workspace_root = root.join("packages/ui");
        let workspaces = vec![WorkspaceInfo {
            root: workspace_root.clone(),
            name: "ui".to_string(),
            is_internal_dependency: false,
        }];
        let root_marker = root.join("node_modules/.package-lock.json");
        let workspace_marker = workspace_root.join("node_modules/.yarn-state.yml");
        write(&root_marker, "a@1\n");
        write(&workspace_marker, "b@1\n");
        let initial = resolution_inputs_hash(&root, &workspaces, &[]);

        write(&root_marker, "a@1\n");
        assert_eq!(
            resolution_inputs_hash(&root, &workspaces, &[]),
            initial,
            "a rewrite with the same content must keep the hash"
        );

        write(&root_marker, "a@2\n");
        let root_changed = resolution_inputs_hash(&root, &workspaces, &[]);
        assert_ne!(root_changed, initial);

        write(&workspace_marker, "b@2\n");
        assert_ne!(
            resolution_inputs_hash(&root, &workspaces, &[]),
            root_changed
        );
    }

    #[test]
    fn lockfile_above_the_root_keys_the_hash() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("repo/packages/app");
        std::fs::create_dir_all(&root).expect("root");
        let lockfile = temp.path().join("repo/pnpm-lock.yaml");
        write(&lockfile, "a: 1\n");
        let initial = resolution_inputs_hash(&root, &[], &[]);
        write(&lockfile, "a: 2\n");
        assert_ne!(resolution_inputs_hash(&root, &[], &[]), initial);
    }

    #[test]
    fn tsconfig_names_match_root_level_variants_only() {
        assert!(is_tsconfig_name(Path::new("tsconfig.json")));
        assert!(is_tsconfig_name(Path::new("tsconfig.app.jsonc")));
        assert!(!is_tsconfig_name(Path::new("package.json")));
        assert!(!is_tsconfig_name(Path::new("tsconfig.ts")));
    }
}
