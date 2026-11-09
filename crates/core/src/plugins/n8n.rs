//! n8n node package plugin.
//!
//! n8n loads the nodes and credentials of a node package from the paths in
//! the `n8n.nodes` and `n8n.credentials` arrays of its `package.json`, so no
//! source file imports them. The paths name build output (for example
//! `dist/nodes/Foo/Foo.node.js`). The plugin maps each path to its source
//! file through the `tsconfig` `outDir` and `rootDir`, and makes it an entry
//! point. A node package often compiles with an `outDir` but no `rootDir`, so
//! the sources sit at the package root next to the output directory. When the
//! `tsconfig` mapping finds no source, the plugin removes the output directory
//! segment and looks for the source file at the package root.
//!
//! n8n reads the members of a node class and a credential class, so the
//! plugin credits these members for a class that implements `INodeType` or
//! `ICredentialType`. Other members of the class stay reportable.

use std::path::{Component, Path, PathBuf};

use fallow_config::{
    EntryPointRole, PackageJson, ScopedUsedClassMemberRule, TsconfigOutputMap,
    TsconfigOutputResolution, UsedClassMemberRule,
};

use super::{Plugin, PluginResult};

/// The keys of the `n8n` block that list the files that n8n loads.
const ENTRY_KEYS: &[&str] = &["nodes", "credentials"];

/// The source extensions that a compiled `.js` node file can come from.
const SOURCE_EXTENSIONS: &[&str] = &["ts", "mts", "cts", "tsx", "js", "mjs", "cjs"];

/// The members of `INodeType` (n8n-workflow) that n8n reads or calls.
const NODE_TYPE_MEMBERS: &[&str] = &[
    "description",
    "methods",
    "webhookMethods",
    "execute",
    "poll",
    "trigger",
    "webhook",
    "supplyData",
    "customOperations",
];

/// The members of `ICredentialType` (n8n-workflow) that n8n reads or calls.
const CREDENTIAL_TYPE_MEMBERS: &[&str] = &[
    "name",
    "displayName",
    "documentationUrl",
    "icon",
    "iconColor",
    "iconUrl",
    "extends",
    "properties",
    "authenticate",
    "preAuthentication",
    "test",
    "genericAuth",
    "httpRequestNode",
    "supportedNodes",
];

/// The interfaces whose members n8n uses, with those members.
const CONTRACTS: &[(&str, &[&str])] = &[
    ("INodeType", NODE_TYPE_MEMBERS),
    ("ICredentialType", CREDENTIAL_TYPE_MEMBERS),
];

pub struct N8nPlugin;

impl Plugin for N8nPlugin {
    fn name(&self) -> &'static str {
        "n8n"
    }

    fn entry_point_role(&self) -> EntryPointRole {
        EntryPointRole::Runtime
    }

    fn used_class_member_rules(&self) -> Vec<UsedClassMemberRule> {
        CONTRACTS
            .iter()
            .map(|(interface, members)| {
                UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                    extends: None,
                    implements: Some((*interface).to_string()),
                    members: members.iter().map(|member| (*member).to_string()).collect(),
                })
            })
            .collect()
    }

    fn is_enabled_with_package_json(&self, pkg: &PackageJson, _root: &Path) -> bool {
        pkg.n8n.is_some()
    }

    fn resolve_package_json(&self, pkg: &PackageJson, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let Some(block) = pkg.n8n.as_ref() else {
            return result;
        };
        let Ok(canonical_root) = dunce::canonicalize(root) else {
            return result;
        };
        let output_map = TsconfigOutputMap::from_project(root);
        let mut entries: Vec<String> = declared_paths(block)
            .filter_map(|entry| {
                let path = entry_source_file(root, entry, &output_map)?;
                root_relative_pattern(&path, root, &canonical_root)
            })
            .collect();
        entries.sort_unstable();
        entries.dedup();
        result.extend_entry_patterns(entries);
        result
    }
}

/// The trimmed, non-empty string values of the `nodes` and `credentials`
/// arrays. Other value shapes give nothing.
fn declared_paths(block: &serde_json::Value) -> impl Iterator<Item = &str> {
    ENTRY_KEYS
        .iter()
        .filter_map(|key| block.get(key).and_then(serde_json::Value::as_array))
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
}

/// The file that n8n loads for a declared path: the path itself when it
/// exists, else the source file that `tsconfig` `outDir` and `rootDir` map it
/// to, else the source file beside the output directory.
fn entry_source_file(root: &Path, entry: &str, output_map: &TsconfigOutputMap) -> Option<PathBuf> {
    let entry = entry.strip_prefix("./").unwrap_or(entry);
    if !Path::new(entry)
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let declared = root.join(entry);
    if declared.is_file() {
        return Some(declared);
    }
    match output_map.resolve_source_for_output_path(&declared, SOURCE_EXTENSIONS) {
        TsconfigOutputResolution::Resolved(source) => Some(source),
        _ => source_beside_output_dir(root, entry),
    }
}

/// The source file of a compiled entry when the sources sit at the package
/// root: `dist/nodes/Foo/Foo.node.js` gives `nodes/Foo/Foo.node.ts`.
fn source_beside_output_dir(root: &Path, entry: &str) -> Option<PathBuf> {
    let (_, rest) = entry.split_once('/')?;
    let stem = rest.strip_suffix(".js")?;
    if stem.is_empty() {
        return None;
    }
    SOURCE_EXTENSIONS
        .iter()
        .map(|extension| root.join(format!("{stem}.{extension}")))
        .find(|candidate| candidate.is_file())
}

fn root_relative_pattern(path: &Path, root: &Path, canonical_root: &Path) -> Option<String> {
    let relative = path
        .strip_prefix(root)
        .or_else(|_| path.strip_prefix(canonical_root))
        .ok()?;
    let pattern = globset::escape(&relative.to_string_lossy().replace('\\', "/"));
    (!pattern.is_empty()).then_some(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_paths_reads_nodes_and_credentials_strings() {
        let block = serde_json::json!({
            "n8nNodesApiVersion": 1,
            "nodes": ["dist/nodes/A.node.js", " ", 7],
            "credentials": ["dist/credentials/B.credentials.js"],
            "other": ["dist/other.js"],
        });
        let paths: Vec<&str> = declared_paths(&block).collect();
        assert_eq!(
            paths,
            vec!["dist/nodes/A.node.js", "dist/credentials/B.credentials.js"]
        );
    }

    #[test]
    fn source_beside_output_dir_maps_to_the_package_root() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("nodes/Foo")).expect("create dir");
        std::fs::write(root.join("nodes/Foo/Foo.node.ts"), "").expect("write file");

        assert_eq!(
            source_beside_output_dir(root, "dist/nodes/Foo/Foo.node.js"),
            Some(root.join("nodes/Foo/Foo.node.ts"))
        );
        assert_eq!(
            source_beside_output_dir(root, "dist/nodes/Bar/Bar.node.js"),
            None
        );
        assert_eq!(source_beside_output_dir(root, "Foo.node.js"), None);
    }

    #[test]
    fn entry_source_file_follows_tsconfig_root_dir() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dunce::canonicalize(dir.path()).expect("canonical root");
        std::fs::create_dir_all(root.join("src/nodes/Foo")).expect("create dir");
        std::fs::write(root.join("src/nodes/Foo/Foo.node.ts"), "").expect("write file");
        std::fs::write(
            root.join("tsconfig.json"),
            r#"{"compilerOptions":{"rootDir":"src","outDir":"dist"}}"#,
        )
        .expect("write tsconfig");
        let output_map = TsconfigOutputMap::from_project(&root);

        assert_eq!(
            entry_source_file(&root, "./dist/nodes/Foo/Foo.node.js", &output_map),
            Some(root.join("src/nodes/Foo/Foo.node.ts"))
        );
        assert_eq!(
            entry_source_file(&root, "../dist/nodes/Foo/Foo.node.js", &output_map),
            None
        );
    }

    #[test]
    fn declared_paths_ignores_a_block_of_another_shape() {
        let block = serde_json::json!({ "nodes": "dist/nodes/A.node.js" });
        assert_eq!(declared_paths(&block).count(), 0);
    }
}
