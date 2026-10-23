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
    ("dotenvx", &["@dotenvx/dotenvx"]),
    ("npmPkgJsonLint", &["npm-package-json-lint"]),
    ("i18next", &["i18next-parser"]),
    ("manypkg", &["@manypkg/cli"]),
    ("rstest", &["@rstest/core"]),
    ("sls", &["serverless", "osls"]),
    ("napi", &["@napi-rs/cli"]),
    ("napi-raw", &["@napi-rs/cli"]),
    ("taplo", &["@taplo/cli"]),
    ("oxnode", &["@oxc-node/cli"]),
    ("ut", &["utoo"]),
    ("utx", &["utoo"]),
    ("vp", &["vite-plus"]),
    ("vpr", &["vite-plus"]),
    ("vpx", &["vite-plus"]),
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

        for bin_name in bin_names(dep_name, &bin) {
            map.insert(bin_name.to_string(), dep_name.clone());
        }
    }

    map
}

/// The binary names that a `bin` field declares: the unscoped package name
/// for the string form, the keys for the object form.
fn bin_names<'a>(package: &'a str, bin: &'a serde_json::Value) -> Vec<&'a str> {
    match bin {
        serde_json::Value::String(_) => vec![package.rsplit('/').next().unwrap_or(package)],
        serde_json::Value::Object(obj) => obj.keys().map(String::as_str).collect(),
        _ => Vec::new(),
    }
}

/// The bin map of a project: the `bin` fields that `node_modules` holds for
/// `dep_names`, plus the `bin` fields of the declared workspace packages.
#[must_use]
pub fn project_bin_map<'a>(
    node_modules_roots: &[&Path],
    dep_names: &[String],
    workspace_packages: impl IntoIterator<Item = &'a fallow_config::PackageJson>,
    declared: &FxHashSet<String>,
) -> FxHashMap<String, String> {
    let mut map = build_bin_to_package_map(node_modules_roots, dep_names);
    add_workspace_binaries(&mut map, workspace_packages, declared);
    map
}

/// Add the `bin` fields of workspace packages to `map`.
///
/// The package manager links the binaries of a workspace package only into
/// the packages that declare it, so a package that no manifest declares adds
/// nothing. An entry that `node_modules` already supplies keeps priority.
/// This lets a script that calls a workspace binary credit its package when
/// `node_modules` is absent.
fn add_workspace_binaries<'a>(
    map: &mut FxHashMap<String, String>,
    packages: impl IntoIterator<Item = &'a fallow_config::PackageJson>,
    declared: &FxHashSet<String>,
) {
    for pkg in packages {
        let (Some(name), Some(bin)) = (pkg.name.as_deref(), pkg.bin.as_ref()) else {
            continue;
        };
        if !declared.contains(name) {
            continue;
        }
        for bin_name in bin_names(name, bin) {
            map.entry(bin_name.to_string())
                .or_insert_with(|| name.to_string());
        }
    }
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

/// Gather sorted, deduped dependency names across the root and workspace
/// manifests.
#[must_use]
pub fn all_dependency_names(manifests: &[&fallow_config::PackageJson]) -> Vec<String> {
    let mut names: Vec<String> = manifests
        .iter()
        .flat_map(|pkg| pkg.all_dependency_names())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Credit each npm alias whose aliased package `used` already holds.
///
/// A script binary resolves to the package that ships it, for example `tsc`
/// to `typescript`. A project that installs that package as
/// `"@typescript/native": "npm:typescript@7"` declares only the alias, so the
/// alias is the dependency in use.
///
/// When `declared` also holds the aliased package under its own name, and
/// that name is not itself an alias, the plain declaration provides the
/// binary and the alias gets no credit. This keeps a side-by-side alias such
/// as `"typescript-5": "npm:typescript@5"` reportable.
pub fn credit_npm_aliases(
    used: &mut FxHashSet<String>,
    manifests: &[&fallow_config::PackageJson],
    declared: &FxHashSet<String>,
) {
    let aliases: Vec<(String, String)> =
        manifests.iter().flat_map(|pkg| pkg.npm_aliases()).collect();
    let is_alias = |name: &str| aliases.iter().any(|(alias, _)| alias == name);
    let credited: Vec<String> = aliases
        .iter()
        .filter(|(_, target)| used.contains(target))
        .filter(|(_, target)| !declared.contains(target) || is_alias(target))
        .map(|(alias, _)| alias.clone())
        .collect();
    used.extend(credited);
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
            ("dotenvx", "@dotenvx/dotenvx"),
            ("npmPkgJsonLint", "npm-package-json-lint"),
            ("i18next", "i18next-parser"),
            ("manypkg", "@manypkg/cli"),
            ("napi", "@napi-rs/cli"),
            ("napi-raw", "@napi-rs/cli"),
            ("taplo", "@taplo/cli"),
            ("oxnode", "@oxc-node/cli"),
            ("ut", "utoo"),
            ("utx", "utoo"),
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

    fn credit(used: &[&str], dev_dependencies: &str) -> FxHashSet<String> {
        let pkg = workspace_package(&format!(r#"{{"devDependencies": {dev_dependencies}}}"#));
        let names: FxHashSet<String> = pkg.all_dependency_names().into_iter().collect();
        let mut used = declared(used);
        credit_npm_aliases(&mut used, &[&pkg], &names);
        used
    }

    #[test]
    fn npm_alias_of_a_used_package_is_credited() {
        let used = credit(
            &["typescript"],
            r#"{"@typescript/native": "npm:typescript@~7.0.2", "idle": "npm:left-pad@1"}"#,
        );
        assert_eq!(used, declared(&["typescript", "@typescript/native"]));
    }

    #[test]
    fn npm_alias_is_credited_when_the_target_name_is_another_alias() {
        let used = credit(
            &["typescript"],
            r#"{"@typescript/native": "npm:typescript@7", "typescript": "npm:@typescript/typescript6@6"}"#,
        );
        assert!(used.contains("@typescript/native"));
    }

    #[test]
    fn npm_alias_is_not_credited_when_the_target_is_declared_plainly() {
        let used = credit(
            &["typescript"],
            r#"{"typescript": "^7.0.2", "typescript-5": "npm:typescript@5"}"#,
        );
        assert_eq!(used, declared(&["typescript"]));
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

    fn workspace_package(json: &str) -> fallow_config::PackageJson {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn workspace_bin_object_form_maps_each_name() {
        let pkg = workspace_package(
            r#"{"name": "@scope/tool", "bin": {"my-tool": "./a.js", "tool-alias": "./b.js"}}"#,
        );
        let mut map = empty_map();
        add_workspace_binaries(&mut map, [&pkg], &declared(&["@scope/tool"]));
        assert_eq!(map.get("my-tool").map(String::as_str), Some("@scope/tool"));
        assert_eq!(
            map.get("tool-alias").map(String::as_str),
            Some("@scope/tool")
        );
    }

    #[test]
    fn workspace_bin_string_form_uses_unscoped_name() {
        let pkg = workspace_package(r#"{"name": "@eggjs/scripts", "bin": "./bin/cli.js"}"#);
        let mut map = empty_map();
        add_workspace_binaries(&mut map, [&pkg], &declared(&["@eggjs/scripts"]));
        assert_eq!(
            map.get("scripts").map(String::as_str),
            Some("@eggjs/scripts")
        );
    }

    #[test]
    fn undeclared_workspace_package_bins_are_skipped() {
        let pkg = workspace_package(r#"{"name": "@scope/tool", "bin": {"my-tool": "./a.js"}}"#);
        let mut map = empty_map();
        add_workspace_binaries(&mut map, [&pkg], &declared(&["other"]));
        assert!(map.is_empty());
    }

    #[test]
    fn node_modules_bin_entry_wins_over_workspace_bin() {
        let pkg = workspace_package(r#"{"name": "@scope/tool", "bin": {"my-tool": "./a.js"}}"#);
        let mut map = empty_map();
        map.insert("my-tool".to_string(), "installed-tool".to_string());
        add_workspace_binaries(&mut map, [&pkg], &declared(&["@scope/tool"]));
        assert_eq!(
            map.get("my-tool").map(String::as_str),
            Some("installed-tool")
        );
    }
}
