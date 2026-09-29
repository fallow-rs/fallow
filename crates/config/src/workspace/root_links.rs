//! Which workspace packages the package manager links into the root
//! `node_modules`.
//!
//! A file of the root package can import a workspace package without a
//! dependency entry only when the install puts a link to that package in the
//! root `node_modules`. The rules per package manager:
//!
//! - npm, yarn classic and bun link every workspace package into the root when
//!   the root `package.json` has a `workspaces` field.
//! - yarn berry does the same only with `nodeLinker: node-modules` and no
//!   `nmHoistingLimits` that keeps packages out of the root. The default PnP
//!   linker and the `pnpm` linker give the root no access to undeclared
//!   workspace packages.
//! - pnpm links a workspace package into the root only with
//!   `node-linker=hoisted`, `shamefully-hoist=true`, or a
//!   `public-hoist-pattern` that matches the package name. The
//!   `hoist-workspace-packages=false` setting turns this off. pnpm reads these
//!   settings from `.npmrc` and from `pnpm-workspace.yaml`.

use std::path::Path;

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
}

impl RootWorkspaceLinks {
    /// Return `true` when the install links the workspace package `name` into
    /// the root `node_modules`.
    #[must_use]
    pub fn links(&self, name: &str) -> bool {
        match self {
            Self::All => true,
            Self::None => false,
            Self::Matching(patterns) => patterns.matches(name),
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
    match manager {
        Manager::Pnpm => pnpm_root_links(root),
        Manager::YarnBerry => yarn_berry_root_links(root, pkg),
        Manager::Hoisting => {
            if pkg.workspace_patterns().is_empty() {
                RootWorkspaceLinks::None
            } else {
                RootWorkspaceLinks::All
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Manager {
    Pnpm,
    YarnBerry,
    /// npm, yarn classic or bun.
    Hoisting,
}

fn detect_manager(root: &Path, pkg: &PackageJson) -> Manager {
    if let Some(field) = pkg.package_manager.as_deref()
        && let Some(manager) = manager_from_field(field)
    {
        return manager;
    }
    if root.join("pnpm-workspace.yaml").is_file() || root.join("pnpm-lock.yaml").is_file() {
        return Manager::Pnpm;
    }
    if root.join(".yarnrc.yml").is_file()
        || root.join(".pnp.cjs").is_file()
        || root.join(".pnp.js").is_file()
    {
        return Manager::YarnBerry;
    }
    Manager::Hoisting
}

/// Read the `packageManager` field, for example `pnpm@9.1.0` or `yarn@4.2.0`.
fn manager_from_field(field: &str) -> Option<Manager> {
    let (name, version) = field.trim().split_once('@')?;
    match name {
        "pnpm" => Some(Manager::Pnpm),
        "yarn" => {
            let major: u32 = version.split('.').next()?.parse().ok()?;
            Some(if major >= 2 {
                Manager::YarnBerry
            } else {
                Manager::Hoisting
            })
        }
        "npm" | "bun" => Some(Manager::Hoisting),
        _ => None,
    }
}

fn yarn_berry_root_links(root: &Path, pkg: &PackageJson) -> RootWorkspaceLinks {
    if pkg.workspace_patterns().is_empty() {
        return RootWorkspaceLinks::None;
    }
    let settings = std::fs::read_to_string(root.join(".yarnrc.yml"))
        .ok()
        .and_then(|content| serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content).ok());
    let setting = |key: &str| -> Option<String> {
        settings
            .as_ref()
            .and_then(|value| value.get(key))
            .and_then(serde_yaml_ng::Value::as_str)
            .map(str::to_owned)
    };
    if setting("nodeLinker").as_deref() != Some("node-modules") {
        return RootWorkspaceLinks::None;
    }
    match setting("nmHoistingLimits").as_deref() {
        Some("workspaces" | "dependencies") => RootWorkspaceLinks::None,
        _ => RootWorkspaceLinks::All,
    }
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

fn pnpm_root_links(root: &Path) -> RootWorkspaceLinks {
    let mut settings = std::fs::read_to_string(root.join(".npmrc"))
        .map(|content| parse_npmrc_hoist_settings(&content))
        .unwrap_or_default();
    // pnpm 10 reads its settings from `pnpm-workspace.yaml` too, and these
    // take priority over `.npmrc`.
    if let Ok(content) = std::fs::read_to_string(root.join("pnpm-workspace.yaml")) {
        settings.merge(parse_pnpm_workspace_hoist_settings(&content));
    }

    if settings.node_linker.as_deref() == Some("hoisted") {
        return RootWorkspaceLinks::All;
    }
    if settings.hoist_workspace_packages == Some(false) {
        return RootWorkspaceLinks::None;
    }
    if settings.shamefully_hoist == Some(true) {
        return RootWorkspaceLinks::All;
    }
    let patterns = HoistPatterns::new(&settings.public_hoist_pattern.unwrap_or_default());
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
        root_workspace_links(dir.path(), &pkg(manifest)).links(name)
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
        assert!(links(
            &[yaml, (".npmrc", "node-linker = \"hoisted\"\n")],
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
        assert!(links(
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
}
