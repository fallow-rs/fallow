//! Binary name → npm package name resolution.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

/// Known binary-name → package-name mappings where they diverge.
///
/// A binary can come from more than one package, for example `run-p` from
/// `npm-run-all` and from its fork `npm-run-all2`. Script analysis credits
/// every candidate, unless `node_modules` shows which candidate installs the
/// binary. Without `node_modules`, this table is the only source that maps a
/// binary to a package whose name is different.
static BINARY_TO_PACKAGE: &[(&str, &[&str])] = &[
    ("tsc", &["typescript"]),
    ("tsserver", &["typescript"]),
    ("tsgo", &["@typescript/native-preview"]),
    ("ng", &["@angular/cli"]),
    ("nuxi", &["nuxt"]),
    ("run-s", &["npm-run-all", "npm-run-all2"]),
    ("run-p", &["npm-run-all", "npm-run-all2"]),
    ("run-s2", &["npm-run-all2"]),
    ("run-p2", &["npm-run-all2"]),
    ("sb", &["storybook"]),
    ("biome", &["@biomejs/biome"]),
    ("oxlint", &["oxlint"]),
    ("ember", &["ember-cli"]),
    ("nest", &["@nestjs/cli"]),
    ("cap", &["@capacitor/cli"]),
    ("cucumber-js", &["@cucumber/cucumber", "cucumber"]),
    ("depcruise", &["dependency-cruiser"]),
    ("depcruise-baseline", &["dependency-cruiser"]),
    ("depcruise-fmt", &["dependency-cruiser"]),
    ("dependency-cruise", &["dependency-cruiser"]),
    ("cz", &["commitizen"]),
    ("git-cz", &["commitizen", "git-cz"]),
    ("flow", &["flow-bin"]),
    ("ncu", &["npm-check-updates"]),
    ("dotenv", &["dotenv-cli"]),
    ("npmPkgJsonLint", &["npm-package-json-lint"]),
    ("i18next", &["i18next-parser"]),
    ("manypkg", &["@manypkg/cli"]),
];

/// The packages that the static table maps `binary` to.
fn static_candidates(binary: &str) -> Option<&'static [&'static str]> {
    BINARY_TO_PACKAGE
        .iter()
        .find(|(bin, _)| *bin == binary)
        .map(|&(_, packages)| packages)
}

/// The package that `node_modules` says installs `binary`: the
/// `node_modules/.bin` link target first, then the `bin` field map.
fn installed_package(
    binary: &str,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
) -> Option<String> {
    let bin_link = root.join("node_modules/.bin").join(binary);
    if let Ok(target) = std::fs::read_link(&bin_link)
        && let Some(pkg_name) = extract_package_from_bin_path(&target)
    {
        return Some(pkg_name);
    }
    bin_map.get(binary).cloned()
}

/// Build a reverse map from binary names to package names by reading each
/// dependency's `package.json` from `node_modules/` and extracting `bin`.
///
/// Probes each provided `node_modules/` root, which covers non-hoisted setups.
#[must_use]
pub fn build_bin_to_package_map(
    node_modules_roots: &[&Path],
    dep_names: &[String],
) -> FxHashMap<String, String> {
    let mut map = FxHashMap::default();

    for dep_name in dep_names {
        let bin = node_modules_roots.iter().find_map(|root| {
            let pkg_path = root
                .join("node_modules")
                .join(dep_name)
                .join("package.json");
            let content = std::fs::read_to_string(&pkg_path).ok()?;
            let pkg = serde_json::from_str::<serde_json::Value>(&content).ok()?;
            pkg.get("bin").cloned()
        });
        let Some(bin) = bin else {
            continue;
        };

        match bin {
            serde_json::Value::String(_) => {
                let bin_name = dep_name.rsplit('/').next().unwrap_or(dep_name);
                map.insert(bin_name.to_string(), dep_name.clone());
            }
            serde_json::Value::Object(ref obj) => {
                for key in obj.keys() {
                    map.insert(key.clone(), dep_name.clone());
                }
            }
            _ => {}
        }
    }

    map
}

/// Resolve a binary name to the npm packages that can provide it.
///
/// The static table wins over `node_modules`, because an unrelated package
/// can also ship a binary with a well-known name. When the table has more than
/// one candidate, the installed package selects one of them. Without a table
/// entry, `node_modules` decides, and the binary name is the fallback.
#[must_use]
pub fn resolve_binary_to_packages(
    binary: &str,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
) -> Vec<String> {
    let Some(candidates) = static_candidates(binary) else {
        let installed = installed_package(binary, root, bin_map);
        return vec![installed.unwrap_or_else(|| binary.to_string())];
    };
    if candidates.len() > 1
        && let Some(pkg_name) = installed_package(binary, root, bin_map)
        && candidates.contains(&pkg_name.as_str())
    {
        return vec![pkg_name];
    }
    candidates.iter().map(|pkg| (*pkg).to_string()).collect()
}

/// Resolve a binary only when it is known to belong to a declared dependency.
#[must_use]
pub fn resolve_known_dependency_binary(
    binary: &str,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    declared_packages: &FxHashSet<String>,
) -> Option<String> {
    if let Some(pkg) = static_candidates(binary)
        .into_iter()
        .flatten()
        .find(|pkg| declared_packages.contains(**pkg))
    {
        return Some((*pkg).to_string());
    }

    let bin_link = root.join("node_modules/.bin").join(binary);
    if let Ok(target) = std::fs::read_link(&bin_link)
        && let Some(pkg_name) = extract_package_from_bin_path(&target)
        && declared_packages.contains(&pkg_name)
    {
        return Some(pkg_name);
    }

    if let Some(pkg_name) = bin_map.get(binary)
        && declared_packages.contains(pkg_name)
    {
        return Some(pkg_name.clone());
    }

    declared_packages
        .contains(binary)
        .then(|| binary.to_string())
}

/// The binary names that the declared dependencies of a project provide.
///
/// Script analysis builds the bin map once. Source analysis uses the same map
/// to resolve a `node_modules/.bin/<name>` path in code.
#[derive(Debug, Clone, Default)]
pub struct DependencyBinaries {
    root: PathBuf,
    bin_map: FxHashMap<String, String>,
    declared_packages: FxHashSet<String>,
}

impl DependencyBinaries {
    #[must_use]
    pub const fn new(
        root: PathBuf,
        bin_map: FxHashMap<String, String>,
        declared_packages: FxHashSet<String>,
    ) -> Self {
        Self {
            root,
            bin_map,
            declared_packages,
        }
    }

    /// Every dependency name the root and workspace manifests declare.
    #[must_use]
    pub const fn declared_packages(&self) -> &FxHashSet<String> {
        &self.declared_packages
    }

    /// The declared dependency that provides `binary`, if one does.
    #[must_use]
    pub fn package_for(&self, binary: &str) -> Option<String> {
        resolve_known_dependency_binary(binary, &self.root, &self.bin_map, &self.declared_packages)
    }
}

/// Extract a package name from a `node_modules/.bin` symlink target path.
///
/// Typical symlink targets:
/// - `../webpack/bin/webpack.js` → `webpack`
/// - `../@babel/cli/bin/babel.js` → `@babel/cli`
pub fn extract_package_from_bin_path(target: &std::path::Path) -> Option<String> {
    let target_str = target.to_string_lossy();
    let parts: Vec<&str> = target_str.split('/').collect();

    for (i, part) in parts.iter().enumerate() {
        if *part == ".." {
            continue;
        }
        if part.starts_with('@') && i + 1 < parts.len() {
            return Some(format!("{}/{}", part, parts[i + 1]));
        }
        return Some(part.to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_map() -> FxHashMap<String, String> {
        FxHashMap::default()
    }

    fn declared(packages: &[&str]) -> FxHashSet<String> {
        packages.iter().map(|pkg| (*pkg).to_string()).collect()
    }

    #[test]
    fn tsserver_maps_to_typescript() {
        let pkg = resolve_binary_to_packages("tsserver", Path::new("/nonexistent"), &empty_map());
        assert_eq!(pkg, ["typescript"]);
    }

    #[test]
    fn nuxi_maps_to_nuxt() {
        let pkg = resolve_binary_to_packages("nuxi", Path::new("/nonexistent"), &empty_map());
        assert_eq!(pkg, ["nuxt"]);
    }

    #[test]
    fn run_p_maps_to_npm_run_all_and_its_fork() {
        let pkg = resolve_binary_to_packages("run-p", Path::new("/nonexistent"), &empty_map());
        assert_eq!(pkg, ["npm-run-all", "npm-run-all2"]);
    }

    #[test]
    fn installed_fork_selects_one_static_candidate() {
        let mut map = FxHashMap::default();
        map.insert("run-p".to_string(), "npm-run-all2".to_string());
        let pkg = resolve_binary_to_packages("run-p", Path::new("/nonexistent"), &map);
        assert_eq!(pkg, ["npm-run-all2"]);
    }

    #[cfg(unix)]
    #[test]
    fn bin_link_selects_one_static_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("node_modules/.bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::os::unix::fs::symlink("../npm-run-all2/bin/run-s/index.js", bin_dir.join("run-s"))
            .unwrap();
        let pkg = resolve_binary_to_packages("run-s", dir.path(), &empty_map());
        assert_eq!(pkg, ["npm-run-all2"]);
    }

    #[test]
    fn unrelated_installed_package_does_not_override_static_candidates() {
        let mut map = FxHashMap::default();
        map.insert("run-p".to_string(), "wrong-package".to_string());
        let pkg = resolve_binary_to_packages("run-p", Path::new("/nonexistent"), &map);
        assert_eq!(pkg, ["npm-run-all", "npm-run-all2"]);
    }

    #[test]
    fn divergent_binaries_resolve_without_node_modules() {
        let cases = [
            ("ember", "ember-cli"),
            ("nest", "@nestjs/cli"),
            ("cap", "@capacitor/cli"),
            ("cucumber-js", "@cucumber/cucumber"),
            ("depcruise", "dependency-cruiser"),
            ("cz", "commitizen"),
            ("git-cz", "commitizen"),
            ("tsgo", "@typescript/native-preview"),
            ("flow", "flow-bin"),
            ("ncu", "npm-check-updates"),
            ("dotenv", "dotenv-cli"),
            ("npmPkgJsonLint", "npm-package-json-lint"),
            ("i18next", "i18next-parser"),
            ("manypkg", "@manypkg/cli"),
        ];
        for (binary, package) in cases {
            let pkg = resolve_binary_to_packages(binary, Path::new("/nonexistent"), &empty_map());
            assert!(
                pkg.iter().any(|candidate| candidate == package),
                "{binary} must resolve to {package}, got {pkg:?}"
            );
        }
    }

    #[test]
    fn known_dependency_binary_accepts_declared_fork() {
        let pkg = resolve_known_dependency_binary(
            "run-p",
            Path::new("/nonexistent"),
            &empty_map(),
            &declared(&["npm-run-all2"]),
        );
        assert_eq!(pkg.as_deref(), Some("npm-run-all2"));
    }

    #[test]
    fn run_s2_maps_to_npm_run_all2() {
        let pkg = resolve_binary_to_packages("run-s2", Path::new("/nonexistent"), &empty_map());
        assert_eq!(pkg, ["npm-run-all2"]);
    }

    #[test]
    fn run_p2_maps_to_npm_run_all2() {
        let pkg = resolve_binary_to_packages("run-p2", Path::new("/nonexistent"), &empty_map());
        assert_eq!(pkg, ["npm-run-all2"]);
    }

    #[test]
    fn sb_maps_to_storybook() {
        let pkg = resolve_binary_to_packages("sb", Path::new("/nonexistent"), &empty_map());
        assert_eq!(pkg, ["storybook"]);
    }

    #[test]
    fn oxlint_maps_to_oxlint() {
        let pkg = resolve_binary_to_packages("oxlint", Path::new("/nonexistent"), &empty_map());
        assert_eq!(pkg, ["oxlint"]);
    }

    #[test]
    fn bin_map_resolves_divergent_binary() {
        let mut map = FxHashMap::default();
        map.insert("attw".to_string(), "@arethetypeswrong/cli".to_string());
        let pkg = resolve_binary_to_packages("attw", Path::new("/nonexistent"), &map);
        assert_eq!(pkg, ["@arethetypeswrong/cli"]);
    }

    #[test]
    fn bin_map_does_not_override_static_table() {
        let mut map = FxHashMap::default();
        map.insert("tsc".to_string(), "wrong-package".to_string());
        let pkg = resolve_binary_to_packages("tsc", Path::new("/nonexistent"), &map);
        assert_eq!(pkg, ["typescript"]);
    }

    #[test]
    fn bin_map_scoped_package_string_bin() {
        let mut map = FxHashMap::default();
        map.insert("my-tool".to_string(), "@scope/my-tool".to_string());
        let pkg = resolve_binary_to_packages("my-tool", Path::new("/nonexistent"), &map);
        assert_eq!(pkg, ["@scope/my-tool"]);
    }

    #[test]
    fn unknown_binary_returns_identity() {
        let pkg =
            resolve_binary_to_packages("some-random-tool", Path::new("/nonexistent"), &empty_map());
        assert_eq!(pkg, ["some-random-tool"]);
    }

    #[test]
    fn known_dependency_binary_accepts_declared_identity() {
        let pkg = resolve_known_dependency_binary(
            "envinfo",
            Path::new("/nonexistent"),
            &empty_map(),
            &declared(&["envinfo"]),
        );
        assert_eq!(pkg.as_deref(), Some("envinfo"));
    }

    #[test]
    fn known_dependency_binary_accepts_static_mapping_when_declared() {
        let pkg = resolve_known_dependency_binary(
            "tsc",
            Path::new("/nonexistent"),
            &empty_map(),
            &declared(&["typescript"]),
        );
        assert_eq!(pkg.as_deref(), Some("typescript"));
    }

    #[test]
    fn known_dependency_binary_accepts_bin_map_when_declared() {
        let mut map = FxHashMap::default();
        map.insert("attw".to_string(), "@arethetypeswrong/cli".to_string());
        let pkg = resolve_known_dependency_binary(
            "attw",
            Path::new("/nonexistent"),
            &map,
            &declared(&["@arethetypeswrong/cli"]),
        );
        assert_eq!(pkg.as_deref(), Some("@arethetypeswrong/cli"));
    }

    #[test]
    fn known_dependency_binary_rejects_identity_fallback_when_not_declared() {
        let pkg = resolve_known_dependency_binary(
            "some-random-tool",
            Path::new("/nonexistent"),
            &empty_map(),
            &FxHashSet::default(),
        );
        assert_eq!(pkg, None);
    }

    #[test]
    fn jest_identity_without_symlink() {
        let pkg = resolve_binary_to_packages("jest", Path::new("/nonexistent"), &empty_map());
        assert_eq!(pkg, ["jest"]);
    }

    #[test]
    fn eslint_identity_without_symlink() {
        let pkg = resolve_binary_to_packages("eslint", Path::new("/nonexistent"), &empty_map());
        assert_eq!(pkg, ["eslint"]);
    }

    #[test]
    fn bin_path_simple_package() {
        let path = std::path::Path::new("../eslint/bin/eslint.js");
        assert_eq!(
            extract_package_from_bin_path(path),
            Some("eslint".to_string())
        );
    }

    #[test]
    fn bin_path_scoped_package() {
        let path = std::path::Path::new("../@angular/cli/bin/ng");
        assert_eq!(
            extract_package_from_bin_path(path),
            Some("@angular/cli".to_string())
        );
    }

    #[test]
    fn bin_path_deeply_nested() {
        let path = std::path::Path::new("../../typescript/bin/tsc");
        assert_eq!(
            extract_package_from_bin_path(path),
            Some("typescript".to_string())
        );
    }

    #[test]
    fn bin_path_no_parent_dots() {
        let path = std::path::Path::new("webpack/bin/webpack.js");
        assert_eq!(
            extract_package_from_bin_path(path),
            Some("webpack".to_string())
        );
    }

    #[test]
    fn bin_path_only_dots() {
        let path = std::path::Path::new("../../..");
        assert_eq!(extract_package_from_bin_path(path), None);
    }

    #[test]
    fn bin_path_scoped_with_multiple_parents() {
        let path = std::path::Path::new("../../../@biomejs/biome/bin/biome");
        assert_eq!(
            extract_package_from_bin_path(path),
            Some("@biomejs/biome".to_string())
        );
    }

    #[test]
    fn bin_map_object_form() {
        let dir = tempfile::tempdir().unwrap();
        let nm = dir.path().join("node_modules/my-cli");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(
            nm.join("package.json"),
            r#"{"name": "my-cli", "bin": {"mycli": "./bin/cli.js", "mc": "./bin/short.js"}}"#,
        )
        .unwrap();

        let map = build_bin_to_package_map(&[dir.path()], &["my-cli".to_string()]);
        assert_eq!(&map["mycli"], "my-cli");
        assert_eq!(&map["mc"], "my-cli");
        assert!(!map.contains_key("my-cli"));
    }

    #[test]
    fn bin_map_string_form_unscoped() {
        let dir = tempfile::tempdir().unwrap();
        let nm = dir.path().join("node_modules/publint");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(
            nm.join("package.json"),
            r#"{"name": "publint", "bin": "./cli.js"}"#,
        )
        .unwrap();

        let map = build_bin_to_package_map(&[dir.path()], &["publint".to_string()]);
        assert_eq!(&map["publint"], "publint");
    }

    #[test]
    fn bin_map_string_form_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let nm = dir.path().join("node_modules/@scope/my-tool");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(
            nm.join("package.json"),
            r#"{"name": "@scope/my-tool", "bin": "./cli.js"}"#,
        )
        .unwrap();

        let map = build_bin_to_package_map(&[dir.path()], &["@scope/my-tool".to_string()]);
        assert_eq!(&map["my-tool"], "@scope/my-tool");
    }

    #[test]
    fn bin_map_missing_node_modules() {
        let map = build_bin_to_package_map(&[Path::new("/nonexistent")], &["foo".to_string()]);
        assert!(map.is_empty());
    }

    #[test]
    fn bin_map_no_bin_field() {
        let dir = tempfile::tempdir().unwrap();
        let nm = dir.path().join("node_modules/lodash");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(
            nm.join("package.json"),
            r#"{"name": "lodash", "main": "index.js"}"#,
        )
        .unwrap();

        let map = build_bin_to_package_map(&[dir.path()], &["lodash".to_string()]);
        assert!(map.is_empty());
    }

    #[test]
    fn bin_map_attw_scenario() {
        let dir = tempfile::tempdir().unwrap();
        let nm = dir.path().join("node_modules/@arethetypeswrong/cli");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(
            nm.join("package.json"),
            r#"{"name": "@arethetypeswrong/cli", "bin": {"attw": "./bin/cli.js"}}"#,
        )
        .unwrap();

        let map = build_bin_to_package_map(&[dir.path()], &["@arethetypeswrong/cli".to_string()]);
        assert_eq!(&map["attw"], "@arethetypeswrong/cli");
    }

    #[test]
    fn bin_map_workspace_fallback() {
        let root = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let ws_nm = ws.path().join("node_modules/my-ws-tool");
        std::fs::create_dir_all(&ws_nm).unwrap();
        std::fs::write(
            ws_nm.join("package.json"),
            r#"{"name": "my-ws-tool", "bin": {"wstool": "./cli.js"}}"#,
        )
        .unwrap();

        let map = build_bin_to_package_map(&[root.path(), ws.path()], &["my-ws-tool".to_string()]);
        assert_eq!(&map["wstool"], "my-ws-tool");
    }
}
