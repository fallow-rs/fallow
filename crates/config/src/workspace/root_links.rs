//! Which workspace packages the package manager links into the root
//! `node_modules`.
//!
//! A file of the root package can import a workspace package without a
//! dependency entry only when the install puts a link to that package in the
//! root `node_modules`. When the root `node_modules` exists, the link on disk
//! decides, because Node.js uses that link at runtime. The one exception is
//! yarn PnP, which does not read `node_modules`.
//!
//! Without an install, the settings of the package manager predict the links:
//!
//! - npm and yarn classic link every workspace package into the root when the
//!   root `package.json` has a `workspaces` field.
//! - bun does the same with the hoisted linker. A workspace install whose
//!   `bun.lock` has `configVersion` 1 or higher uses the isolated linker, which
//!   links no workspace package into the root. `install.linker` in
//!   `bunfig.toml` overrides this.
//! - yarn berry links every workspace package only with
//!   `nodeLinker: node-modules` and no `nmHoistingLimits` that keeps packages
//!   out of the root. The default PnP linker and the `pnpm` linker give the
//!   root no access to undeclared workspace packages.
//! - pnpm links a workspace package into the root only with
//!   `shamefully-hoist=true` or a `public-hoist-pattern` that matches the
//!   package name. Before pnpm 10, the default pattern is `*eslint*` and
//!   `*prettier*`. `hoist-workspace-packages=false` and `node-linker=hoisted`
//!   give no root link. pnpm before 11 reads these settings from `.npmrc`, and
//!   pnpm 10 and later read them from `pnpm-workspace.yaml`. When the
//!   `packageManager` field gives no pnpm version, both files count.

use std::path::{Path, PathBuf};

use globset::{Glob, GlobMatcher};

use super::package_json::PackageJson;

/// The workspace packages that a root file can import without a declaration.
#[derive(Debug, Clone)]
pub enum RootWorkspaceLinks {
    /// The install links every workspace package into the root.
    All,
    /// The install links no workspace package into the root.
    None,
    /// The install links the workspace packages that match a pnpm
    /// `public-hoist-pattern` into the root.
    Matching(HoistPatterns),
    /// The root `node_modules` directory at this path exists. A workspace
    /// package is linked when `node_modules/<name>` points at its root.
    Installed(PathBuf),
}

impl RootWorkspaceLinks {
    /// Return `true` when the install links the workspace package `name`, with
    /// its root directory at `package_root`, into the root `node_modules`.
    #[must_use]
    pub fn links(&self, name: &str, package_root: &Path) -> bool {
        match self {
            Self::All => true,
            Self::None => false,
            Self::Matching(patterns) => patterns.matches(name),
            Self::Installed(node_modules) => {
                let Ok(link) = dunce::canonicalize(node_modules.join(name)) else {
                    return false;
                };
                dunce::canonicalize(package_root).is_ok_and(|package_root| link == package_root)
            }
        }
    }
}

/// Compiled pnpm `public-hoist-pattern` entries.
#[derive(Debug, Clone, Default)]
pub struct HoistPatterns {
    include: Vec<GlobMatcher>,
    exclude: Vec<GlobMatcher>,
}

impl HoistPatterns {
    fn new(patterns: &[String]) -> Self {
        let mut compiled = Self::default();
        for pattern in patterns {
            let pattern = pattern.trim();
            let (target, negated) = match pattern.strip_prefix('!') {
                Some(rest) => (&mut compiled.exclude, rest),
                None => (&mut compiled.include, pattern),
            };
            if let Ok(glob) = Glob::new(negated) {
                target.push(glob.compile_matcher());
            }
        }
        compiled
    }

    fn is_empty(&self) -> bool {
        self.include.is_empty()
    }

    fn matches(&self, name: &str) -> bool {
        self.include.iter().any(|glob| glob.is_match(name))
            && !self.exclude.iter().any(|glob| glob.is_match(name))
    }
}

/// Find which workspace packages the package manager of the project at `root`
/// links into the root `node_modules`.
#[must_use]
pub fn root_workspace_links(root: &Path, pkg: &PackageJson) -> RootWorkspaceLinks {
    let manager = detect_manager(root, pkg);
    if manager == Manager::YarnBerry && yarn_berry_uses_pnp(root) {
        return RootWorkspaceLinks::None;
    }
    let node_modules = root.join("node_modules");
    if node_modules.is_dir() {
        return RootWorkspaceLinks::Installed(node_modules);
    }
    match manager {
        Manager::Pnpm { major } => pnpm_root_links(root, major),
        Manager::YarnBerry => yarn_berry_root_links(root, pkg),
        Manager::Bun => bun_root_links(root, pkg),
        Manager::Hoisting => hoisting_root_links(pkg),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Manager {
    /// pnpm, with the major version when the `packageManager` field gives one.
    Pnpm {
        major: Option<u32>,
    },
    YarnBerry,
    Bun,
    /// npm or yarn classic.
    Hoisting,
}

fn detect_manager(root: &Path, pkg: &PackageJson) -> Manager {
    if let Some(field) = pkg.package_manager.as_deref()
        && let Some(manager) = manager_from_field(field)
    {
        return manager;
    }
    if root.join("pnpm-workspace.yaml").is_file() || root.join("pnpm-lock.yaml").is_file() {
        return Manager::Pnpm { major: None };
    }
    if root.join(".yarnrc.yml").is_file()
        || root.join(".pnp.cjs").is_file()
        || root.join(".pnp.js").is_file()
    {
        return Manager::YarnBerry;
    }
    if root.join("bun.lock").is_file() || root.join("bun.lockb").is_file() {
        return Manager::Bun;
    }
    Manager::Hoisting
}

/// Read the `packageManager` field, for example `pnpm@9.1.0` or `yarn@4.2.0`.
fn manager_from_field(field: &str) -> Option<Manager> {
    let (name, version) = field.trim().split_once('@')?;
    let major = version
        .split(['.', '+', '-'])
        .next()
        .and_then(|major| major.parse::<u32>().ok());
    match name {
        "pnpm" => Some(Manager::Pnpm { major }),
        "yarn" => Some(if major? >= 2 {
            Manager::YarnBerry
        } else {
            Manager::Hoisting
        }),
        "bun" => Some(Manager::Bun),
        "npm" => Some(Manager::Hoisting),
        _ => None,
    }
}

fn hoisting_root_links(pkg: &PackageJson) -> RootWorkspaceLinks {
    if pkg.workspace_patterns().is_empty() {
        RootWorkspaceLinks::None
    } else {
        RootWorkspaceLinks::All
    }
}

fn read_yarnrc(root: &Path) -> Option<serde_yaml_ng::Value> {
    let content = std::fs::read_to_string(root.join(".yarnrc.yml")).ok()?;
    serde_yaml_ng::from_str(&content).ok()
}

fn yarnrc_setting(settings: Option<&serde_yaml_ng::Value>, key: &str) -> Option<String> {
    settings
        .and_then(|value| value.get(key))
        .and_then(serde_yaml_ng::Value::as_str)
        .map(str::to_owned)
}

/// Return `true` when yarn berry uses Plug'n'Play, its default linker. The PnP
/// runtime does not read `node_modules`, so a stale root link has no effect.
fn yarn_berry_uses_pnp(root: &Path) -> bool {
    let settings = read_yarnrc(root);
    !matches!(
        yarnrc_setting(settings.as_ref(), "nodeLinker").as_deref(),
        Some("node-modules" | "pnpm")
    )
}

fn yarn_berry_root_links(root: &Path, pkg: &PackageJson) -> RootWorkspaceLinks {
    if pkg.workspace_patterns().is_empty() {
        return RootWorkspaceLinks::None;
    }
    let settings = read_yarnrc(root);
    if yarnrc_setting(settings.as_ref(), "nodeLinker").as_deref() != Some("node-modules") {
        return RootWorkspaceLinks::None;
    }
    match yarnrc_setting(settings.as_ref(), "nmHoistingLimits").as_deref() {
        Some("workspaces" | "dependencies") => RootWorkspaceLinks::None,
        _ => RootWorkspaceLinks::All,
    }
}

/// The first `bun.lock` `configVersion` that records the linker choice. A
/// workspace install with this version or later uses the isolated linker.
const BUN_ISOLATED_CONFIG_VERSION: u64 = 1;

fn bun_root_links(root: &Path, pkg: &PackageJson) -> RootWorkspaceLinks {
    if pkg.workspace_patterns().is_empty() {
        return RootWorkspaceLinks::None;
    }
    match bunfig_linker(root).as_deref() {
        Some("isolated") => return RootWorkspaceLinks::None,
        Some("hoisted") => return RootWorkspaceLinks::All,
        _ => {}
    }
    let config_version = std::fs::read_to_string(root.join("bun.lock"))
        .ok()
        .and_then(|content| bun_lock_config_version(&content));
    match config_version {
        Some(version) if version >= BUN_ISOLATED_CONFIG_VERSION => RootWorkspaceLinks::None,
        _ => RootWorkspaceLinks::All,
    }
}

/// Read `install.linker` from `bunfig.toml`.
fn bunfig_linker(root: &Path) -> Option<String> {
    let content = std::fs::read_to_string(root.join("bunfig.toml")).ok()?;
    let value: toml::Table = toml::from_str(&content).ok()?;
    value
        .get("install")?
        .get("linker")?
        .as_str()
        .map(str::to_owned)
}

/// Read the top-level `configVersion` of a `bun.lock` file, which is JSON with
/// trailing commas.
fn bun_lock_config_version(content: &str) -> Option<u64> {
    let value: serde_json::Value = crate::jsonc::parse_to_value(content).ok()?;
    value.get("configVersion")?.as_u64()
}

/// pnpm settings that control which packages get a root link.
#[derive(Debug, Default)]
struct PnpmHoistSettings {
    node_linker: Option<String>,
    shamefully_hoist: Option<bool>,
    public_hoist_pattern: Option<Vec<String>>,
    hoist_workspace_packages: Option<bool>,
}

impl PnpmHoistSettings {
    /// Take each setting from `other` when `other` sets it.
    fn merge(&mut self, other: Self) {
        if other.node_linker.is_some() {
            self.node_linker = other.node_linker;
        }
        if other.shamefully_hoist.is_some() {
            self.shamefully_hoist = other.shamefully_hoist;
        }
        if other.public_hoist_pattern.is_some() {
            self.public_hoist_pattern = other.public_hoist_pattern;
        }
        if other.hoist_workspace_packages.is_some() {
            self.hoist_workspace_packages = other.hoist_workspace_packages;
        }
    }
}

/// The first pnpm major that reads its settings from `pnpm-workspace.yaml`
/// and has an empty default `public-hoist-pattern`.
const PNPM_WORKSPACE_YAML_MAJOR: u32 = 10;
/// The first pnpm major that ignores the hoisting settings in `.npmrc`.
const PNPM_NPMRC_IGNORED_MAJOR: u32 = 11;
/// The default `public-hoist-pattern` before pnpm 10.
const PNPM_9_DEFAULT_PUBLIC_HOIST_PATTERN: [&str; 2] = ["*eslint*", "*prettier*"];

/// Predict the root links of a pnpm install from its settings. When `major` is
/// not known, read both settings files and use the pnpm 9 defaults.
fn pnpm_root_links(root: &Path, major: Option<u32>) -> RootWorkspaceLinks {
    let reads_npmrc = major.is_none_or(|major| major < PNPM_NPMRC_IGNORED_MAJOR);
    let reads_workspace_yaml = major.is_none_or(|major| major >= PNPM_WORKSPACE_YAML_MAJOR);
    let has_pnpm_9_defaults = major.is_none_or(|major| major < PNPM_WORKSPACE_YAML_MAJOR);

    let mut settings = PnpmHoistSettings::default();
    if reads_npmrc && let Ok(content) = std::fs::read_to_string(root.join(".npmrc")) {
        settings.merge(parse_npmrc_hoist_settings(&content));
    }
    // `pnpm-workspace.yaml` settings take priority over `.npmrc`.
    if reads_workspace_yaml
        && let Ok(content) = std::fs::read_to_string(root.join("pnpm-workspace.yaml"))
    {
        settings.merge(parse_pnpm_workspace_hoist_settings(&content));
    }

    // The hoisted linker puts the workspace links in the `node_modules` of each
    // package, not in the root.
    if settings.node_linker.as_deref() == Some("hoisted")
        || settings.hoist_workspace_packages == Some(false)
    {
        return RootWorkspaceLinks::None;
    }
    if settings.shamefully_hoist == Some(true) {
        return RootWorkspaceLinks::All;
    }
    let patterns = settings.public_hoist_pattern.unwrap_or_else(|| {
        if has_pnpm_9_defaults {
            PNPM_9_DEFAULT_PUBLIC_HOIST_PATTERN
                .iter()
                .map(|pattern| (*pattern).to_owned())
                .collect()
        } else {
            Vec::new()
        }
    });
    let patterns = HoistPatterns::new(&patterns);
    if patterns.is_empty() {
        RootWorkspaceLinks::None
    } else {
        RootWorkspaceLinks::Matching(patterns)
    }
}

fn parse_npmrc_hoist_settings(content: &str) -> PnpmHoistSettings {
    let mut settings = PnpmHoistSettings::default();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = unquote(value.trim());
        match key {
            "node-linker" => settings.node_linker = Some(value.to_owned()),
            "shamefully-hoist" => settings.shamefully_hoist = Some(value == "true"),
            "hoist-workspace-packages" => {
                settings.hoist_workspace_packages = Some(value != "false");
            }
            "public-hoist-pattern[]" => settings
                .public_hoist_pattern
                .get_or_insert_with(Vec::new)
                .push(value.to_owned()),
            "public-hoist-pattern" => {
                settings.public_hoist_pattern = Some(
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|pattern| !pattern.is_empty())
                        .map(str::to_owned)
                        .collect(),
                );
            }
            _ => {}
        }
    }
    settings
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn parse_pnpm_workspace_hoist_settings(content: &str) -> PnpmHoistSettings {
    let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(content) else {
        return PnpmHoistSettings::default();
    };
    let public_hoist_pattern =
        value
            .get("publicHoistPattern")
            .and_then(|patterns| match patterns {
                serde_yaml_ng::Value::String(pattern) => Some(vec![pattern.clone()]),
                serde_yaml_ng::Value::Sequence(items) => Some(
                    items
                        .iter()
                        .filter_map(serde_yaml_ng::Value::as_str)
                        .map(str::to_owned)
                        .collect(),
                ),
                _ => None,
            });
    PnpmHoistSettings {
        node_linker: value
            .get("nodeLinker")
            .and_then(serde_yaml_ng::Value::as_str)
            .map(str::to_owned),
        shamefully_hoist: value
            .get("shamefullyHoist")
            .and_then(serde_yaml_ng::Value::as_bool),
        public_hoist_pattern,
        hoist_workspace_packages: value
            .get("hoistWorkspacePackages")
            .and_then(serde_yaml_ng::Value::as_bool),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(json: &str) -> PackageJson {
        serde_json::from_str(json).expect("valid package.json")
    }

    fn links(files: &[(&str, &str)], manifest: &str, name: &str) -> bool {
        let dir = tempfile::tempdir().expect("temp dir");
        for (path, content) in files {
            std::fs::write(dir.path().join(path), content).expect("write file");
        }
        root_workspace_links(dir.path(), &pkg(manifest)).links(name, dir.path())
    }

    const WORKSPACES: &str = r#"{ "workspaces": ["packages/*"] }"#;
    const NO_WORKSPACES: &str = "{}";

    #[test]
    fn npm_yarn_classic_and_bun_link_every_workspace_package() {
        assert!(links(&[("package-lock.json", "{}")], WORKSPACES, "@a/b"));
        assert!(links(&[("yarn.lock", "")], WORKSPACES, "@a/b"));
        assert!(links(&[("bun.lock", "{}")], WORKSPACES, "@a/b"));
        assert!(!links(
            &[("package-lock.json", "{}")],
            NO_WORKSPACES,
            "@a/b"
        ));
    }

    #[test]
    fn package_manager_field_picks_the_manager() {
        let yarn4 = r#"{ "packageManager": "yarn@4.1.0", "workspaces": ["p/*"] }"#;
        assert!(!links(&[], yarn4, "@a/b"));
        let yarn1 = r#"{ "packageManager": "yarn@1.22.22", "workspaces": ["p/*"] }"#;
        assert!(links(&[], yarn1, "@a/b"));
        let pnpm = r#"{ "packageManager": "pnpm@9.0.0", "workspaces": ["p/*"] }"#;
        assert!(!links(&[], pnpm, "@a/b"));
    }

    #[test]
    fn yarn_berry_links_only_with_the_node_modules_linker() {
        let rc = |content: &'static str| [(".yarnrc.yml", content)];
        assert!(!links(&rc("enableGlobalCache: true\n"), WORKSPACES, "@a/b"));
        assert!(!links(&rc("nodeLinker: pnp\n"), WORKSPACES, "@a/b"));
        assert!(!links(&rc("nodeLinker: pnpm\n"), WORKSPACES, "@a/b"));
        assert!(links(&rc("nodeLinker: node-modules\n"), WORKSPACES, "@a/b"));
        assert!(!links(
            &rc("nodeLinker: node-modules\nnmHoistingLimits: workspaces\n"),
            WORKSPACES,
            "@a/b"
        ));
    }

    #[test]
    fn pnpm_links_only_with_a_hoisting_setting() {
        let yaml = ("pnpm-workspace.yaml", "packages:\n  - packages/*\n");
        assert!(!links(&[yaml], NO_WORKSPACES, "@a/b"));
        assert!(!links(&[yaml], WORKSPACES, "@a/b"));
        assert!(links(
            &[yaml, (".npmrc", "shamefully-hoist=true\n")],
            NO_WORKSPACES,
            "@a/b"
        ));
        // The hoisted linker puts the workspace links in each package, not in
        // the root.
        assert!(!links(
            &[
                yaml,
                (
                    ".npmrc",
                    "node-linker = \"hoisted\"\nshamefully-hoist=true\n"
                )
            ],
            NO_WORKSPACES,
            "@a/b"
        ));
        assert!(!links(
            &[
                yaml,
                (
                    ".npmrc",
                    "shamefully-hoist=true\nhoist-workspace-packages=false\n"
                )
            ],
            NO_WORKSPACES,
            "@a/b"
        ));
        assert!(!links(
            &[(
                "pnpm-workspace.yaml",
                "packages: [packages/*]\nnodeLinker: hoisted\n"
            )],
            NO_WORKSPACES,
            "@a/b"
        ));
        // `pnpm-workspace.yaml` settings take priority over `.npmrc`.
        assert!(!links(
            &[
                (
                    "pnpm-workspace.yaml",
                    "packages: [packages/*]\nshamefullyHoist: false\n"
                ),
                (".npmrc", "shamefully-hoist=true\n"),
            ],
            NO_WORKSPACES,
            "@a/b"
        ));
    }

    #[test]
    fn pnpm_public_hoist_pattern_links_matching_packages() {
        let lock = ("pnpm-lock.yaml", "lockfileVersion: '9.0'\n");
        let npmrc = (
            ".npmrc",
            "public-hoist-pattern[]=@a/*\npublic-hoist-pattern[]=!@a/private\n",
        );
        assert!(links(&[lock, npmrc], NO_WORKSPACES, "@a/b"));
        assert!(!links(&[lock, npmrc], NO_WORKSPACES, "@a/private"));
        assert!(!links(&[lock, npmrc], NO_WORKSPACES, "@c/d"));
        assert!(links(
            &[lock, (".npmrc", "public-hoist-pattern=*\n")],
            NO_WORKSPACES,
            "x"
        ));
        let yaml = ("pnpm-workspace.yaml", "publicHoistPattern:\n  - \"@a/*\"\n");
        assert!(links(&[yaml], NO_WORKSPACES, "@a/b"));
        assert!(!links(&[yaml], NO_WORKSPACES, "@c/d"));
    }

    #[test]
    fn pnpm_9_hoists_eslint_and_prettier_packages_by_default() {
        let yaml = ("pnpm-workspace.yaml", "packages:\n  - packages/*\n");
        let pnpm = |version: &str| format!(r#"{{ "packageManager": "pnpm@{version}" }}"#);
        for manifest in [pnpm("9.6.0"), NO_WORKSPACES.to_owned()] {
            assert!(links(&[yaml], &manifest, "@repo/eslint-config"));
            assert!(links(&[yaml], &manifest, "prettier-config-repo"));
            assert!(!links(&[yaml], &manifest, "@repo/lib"));
        }
        // pnpm 10 has an empty default pattern.
        assert!(!links(&[yaml], &pnpm("10.34.5"), "@repo/eslint-config"));
        // A configured pattern replaces the default.
        assert!(!links(
            &[yaml, (".npmrc", "public-hoist-pattern[]=@x/*\n")],
            &pnpm("9.6.0"),
            "@repo/eslint-config"
        ));
    }

    #[test]
    fn pnpm_version_picks_the_settings_file() {
        let pnpm = |version: &str| format!(r#"{{ "packageManager": "pnpm@{version}" }}"#);
        let npmrc = (".npmrc", "shamefully-hoist=true\n");
        let yaml = (
            "pnpm-workspace.yaml",
            "packages: [packages/*]\nshamefullyHoist: true\n",
        );
        let plain_yaml = ("pnpm-workspace.yaml", "packages: [packages/*]\n");
        // pnpm 9 reads `.npmrc` only.
        assert!(links(&[plain_yaml, npmrc], &pnpm("9.6.0"), "@a/b"));
        assert!(!links(&[yaml], &pnpm("9.6.0"), "@a/b"));
        // pnpm 10 reads both files.
        assert!(links(&[plain_yaml, npmrc], &pnpm("10.34.5"), "@a/b"));
        assert!(links(&[yaml], &pnpm("10.34.5"), "@a/b"));
        // pnpm 11 reads `pnpm-workspace.yaml` only.
        assert!(!links(&[plain_yaml, npmrc], &pnpm("11.25.0"), "@a/b"));
        assert!(links(&[yaml], &pnpm("11.25.0+sha512.abc"), "@a/b"));
    }

    #[test]
    fn bun_isolated_linker_links_no_workspace_package() {
        let lock = |config_version: u32| {
            (
                "bun.lock",
                format!(
                    "{{\n  \"lockfileVersion\": 1,\n  \"configVersion\": {config_version},\n  \"workspaces\": {{}},\n}}\n"
                ),
            )
        };
        let (lock_name, isolated) = lock(1);
        let (_, hoisted) = lock(0);
        assert!(!links(
            &[(lock_name, isolated.as_str())],
            WORKSPACES,
            "@a/b"
        ));
        assert!(links(&[(lock_name, hoisted.as_str())], WORKSPACES, "@a/b"));
        assert!(links(
            &[
                (lock_name, isolated.as_str()),
                ("bunfig.toml", "[install]\nlinker = \"hoisted\"\n")
            ],
            WORKSPACES,
            "@a/b"
        ));
        assert!(!links(
            &[
                (lock_name, hoisted.as_str()),
                ("bunfig.toml", "[install]\nlinker = \"isolated\"\n")
            ],
            WORKSPACES,
            "@a/b"
        ));
        let bun = r#"{ "packageManager": "bun@1.4.2", "workspaces": ["p/*"] }"#;
        assert!(!links(&[(lock_name, isolated.as_str())], bun, "@a/b"));
    }

    #[test]
    fn installed_root_link_decides() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let package_root = root.join("packages/eslint-config");
        std::fs::create_dir_all(&package_root).expect("create package");
        std::fs::create_dir_all(root.join("packages/lib")).expect("create package");
        std::fs::create_dir_all(root.join("node_modules/@repo")).expect("create scope");
        std::fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").expect("lock");
        std::fs::write(root.join(".npmrc"), "shamefully-hoist=true\n").expect("npmrc");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&package_root, root.join("node_modules/@repo/eslint-config"))
            .expect("symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(
            &package_root,
            root.join("node_modules/@repo/eslint-config"),
        )
        .expect("symlink");

        let links = root_workspace_links(root, &pkg(NO_WORKSPACES));
        assert!(links.links("@repo/eslint-config", &package_root));
        // The settings predict a link, but the install has none.
        assert!(!links.links("@repo/lib", &root.join("packages/lib")));
        // A link that points somewhere else does not count.
        assert!(!links.links("@repo/eslint-config", &root.join("packages/lib")));
    }

    #[test]
    fn yarn_pnp_ignores_the_root_node_modules() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("node_modules/@a/b")).expect("create dir");
        std::fs::write(root.join(".yarnrc.yml"), "nodeLinker: pnp\n").expect("yarnrc");
        let links = root_workspace_links(root, &pkg(WORKSPACES));
        assert!(!links.links("@a/b", &root.join("node_modules/@a/b")));
    }
}
