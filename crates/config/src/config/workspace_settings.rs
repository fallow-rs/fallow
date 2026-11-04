//! Resolved `workspaces.settings`: per-workspace `ignoreDependencies` and
//! `includeEntryExports`.
//!
//! A settings key names a workspace root or a glob over workspace roots. The
//! workspace set of a run is known only after workspace discovery, so the
//! analysis binds the keys to the discovered workspaces once per pass
//! ([`ResolvedWorkspaceSettings::bind`]). Before a bind, no key selects a
//! workspace and only the global settings apply.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use globset::GlobMatcher;
use rustc_hash::FxHashSet;

use super::dependency_ignore::IgnoreDependencyMatcher;
use super::glob_validation::compile_path_glob;
use crate::workspace::{WorkspaceInfo, WorkspaceSettings};

/// How a settings key selects workspace roots.
#[derive(Debug, Clone)]
enum Selector {
    /// A root without glob characters, compared as a path.
    Exact(String),
    /// A glob over project-root-relative workspace roots.
    Glob {
        matcher: GlobMatcher,
        /// Path segments without glob characters. A glob with more literal
        /// segments is more specific.
        literal_segments: usize,
    },
}

/// The precedence of a key for `includeEntryExports`: an exact root wins over
/// any glob, and a glob with more literal segments wins over a shorter glob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Specificity {
    Glob(usize),
    Exact,
}

impl Selector {
    fn new(key: &str) -> Option<Self> {
        let normalized = normalize_root(key);
        if !normalized.contains(['*', '?', '[', '{']) {
            return Some(Self::Exact(normalized));
        }
        let matcher = compile_path_glob(&normalized).ok()?.compile_matcher();
        let literal_segments = normalized
            .split('/')
            .filter(|segment| !segment.contains(['*', '?', '[', '{']))
            .count();
        Some(Self::Glob {
            matcher,
            literal_segments,
        })
    }

    fn is_project_root(&self) -> bool {
        matches!(self, Self::Exact(root) if root == ".")
    }

    fn selects(&self, relative_root: &str) -> bool {
        match self {
            Self::Exact(root) => root == relative_root,
            Self::Glob { matcher, .. } => matcher.is_match(relative_root),
        }
    }

    const fn specificity(&self) -> Specificity {
        match self {
            Self::Exact(_) => Specificity::Exact,
            Self::Glob {
                literal_segments, ..
            } => Specificity::Glob(*literal_segments),
        }
    }
}

/// Use `/` as the separator and drop a leading `./` and a trailing `/`, so a
/// key compares with a root as `fallow list --workspaces` prints it.
fn normalize_root(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let path = path.trim_start_matches("./").trim_end_matches('/');
    if path.is_empty() {
        ".".to_string()
    } else {
        path.to_string()
    }
}

#[derive(Debug, Clone)]
struct SettingsKey {
    key: String,
    selector: Option<Selector>,
    ignore_dependencies: IgnoreDependencyMatcher,
    include_entry_exports: Option<bool>,
}

/// The effective settings of one discovered workspace.
#[derive(Debug)]
struct BoundWorkspace {
    root: PathBuf,
    /// Indices of the keys that select this workspace, in key order.
    keys: Vec<usize>,
    include_entry_exports: Option<bool>,
}

/// Discovered workspaces with at least one selecting key, deepest root first.
#[derive(Debug, Default)]
struct BoundWorkspaces {
    workspaces: Vec<BoundWorkspace>,
}

/// Compiled `workspaces.settings`.
#[derive(Debug, Default)]
pub struct ResolvedWorkspaceSettings {
    keys: Arc<[SettingsKey]>,
    /// Set when a CLI flag or a tool parameter turns `includeEntryExports` on
    /// for one run. It wins over every per-workspace value.
    include_entry_exports_forced: bool,
    bound: Arc<RwLock<Arc<BoundWorkspaces>>>,
}

/// A clone gets its own bound slot, with a copy of the current binding. A
/// pass on one clone (an engine session, an audit base pass) then never
/// rebinds the workspaces that a pass on another clone reads.
impl Clone for ResolvedWorkspaceSettings {
    fn clone(&self) -> Self {
        Self {
            keys: Arc::clone(&self.keys),
            include_entry_exports_forced: self.include_entry_exports_forced,
            bound: Arc::new(RwLock::new(self.bound())),
        }
    }
}

impl ResolvedWorkspaceSettings {
    /// Compile the configured settings keys. A key whose glob does not
    /// compile selects no workspace; config loading rejects such a key first.
    #[must_use]
    pub fn compile(settings: &BTreeMap<String, WorkspaceSettings>) -> Self {
        let keys: Vec<SettingsKey> = settings
            .iter()
            .map(|(key, value)| SettingsKey {
                key: key.clone(),
                selector: Selector::new(key),
                ignore_dependencies: IgnoreDependencyMatcher::compile(&value.ignore_dependencies),
                include_entry_exports: value.include_entry_exports,
            })
            .collect();
        Self {
            keys: keys.into(),
            include_entry_exports_forced: false,
            bound: Arc::default(),
        }
    }

    /// Whether no settings key was configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Make `includeEntryExports` true in every workspace for this run.
    pub const fn force_include_entry_exports(&mut self) {
        self.include_entry_exports_forced = true;
    }

    /// Whether any key turns `includeEntryExports` on.
    #[must_use]
    pub fn any_include_entry_exports(&self) -> bool {
        self.keys
            .iter()
            .any(|key| key.include_entry_exports == Some(true))
    }

    /// Bind the keys to the workspaces of this analysis pass.
    ///
    /// Warn once per key that selects no workspace, and once per workspace
    /// where two keys of the same precedence give different
    /// `includeEntryExports` values (the global value then applies).
    pub fn bind(&self, root: &Path, workspaces: &[WorkspaceInfo]) {
        if self.keys.is_empty() {
            return;
        }
        let mut selected = vec![false; self.keys.len()];
        let mut bound = Vec::new();
        for workspace in workspaces {
            let relative = workspace
                .root
                .strip_prefix(root)
                .unwrap_or(&workspace.root)
                .to_string_lossy()
                .replace('\\', "/");
            if relative.is_empty() {
                continue;
            }
            let keys: Vec<usize> = self
                .keys
                .iter()
                .enumerate()
                .filter(|(_, key)| key.selector.as_ref().is_some_and(|s| s.selects(&relative)))
                .map(|(index, _)| index)
                .collect();
            if keys.is_empty() {
                continue;
            }
            for &index in &keys {
                selected[index] = true;
            }
            let include_entry_exports = self.include_value(&keys, &relative);
            bound.push(BoundWorkspace {
                root: workspace.root.clone(),
                keys,
                include_entry_exports,
            });
        }
        for (key, was_selected) in self.keys.iter().zip(selected) {
            if !was_selected && key.selector.as_ref().is_some_and(Selector::is_project_root) {
                warn_once(&format!(
                    "workspaces.settings key '{}' names the project root; the root package \
                     uses only the global keys, so set them at the top level.",
                    key.key
                ));
            } else if !was_selected {
                warn_once(&format!(
                    "workspaces.settings key '{}' selects no workspace; its settings have no \
                     effect. Use a root as `fallow list --workspaces` prints it.",
                    key.key
                ));
            }
        }
        bound.sort_by_key(|workspace| std::cmp::Reverse(workspace.root.components().count()));
        if let Ok(mut slot) = self.bound.write() {
            *slot = Arc::new(BoundWorkspaces { workspaces: bound });
        }
    }

    /// The `includeEntryExports` value of the most specific key that sets
    /// one, or `None` when no key sets one or two equal keys disagree.
    fn include_value(&self, keys: &[usize], relative_root: &str) -> Option<bool> {
        let mut best: Option<(Specificity, bool)> = None;
        let mut tie = false;
        for &index in keys {
            let key = &self.keys[index];
            let (Some(value), Some(selector)) = (key.include_entry_exports, &key.selector) else {
                continue;
            };
            let specificity = selector.specificity();
            match best {
                Some((current, _)) if specificity < current => {}
                Some((current, current_value)) if specificity == current => {
                    tie |= current_value != value;
                }
                _ => {
                    best = Some((specificity, value));
                    tie = false;
                }
            }
        }
        if tie {
            warn_once(&format!(
                "workspaces.settings keys of the same precedence give different \
                 includeEntryExports values for workspace '{relative_root}'; the global \
                 includeEntryExports value applies there."
            ));
            return None;
        }
        best.map(|(_, value)| value)
    }

    fn bound(&self) -> Arc<BoundWorkspaces> {
        self.bound
            .read()
            .map_or_else(|_| Arc::default(), |slot| Arc::clone(&slot))
    }

    /// The `includeEntryExports` value for the file at `path`, from the
    /// deepest bound workspace that contains it, or `global` when no key sets
    /// a value there.
    #[must_use]
    pub fn include_entry_exports_for(&self, path: &Path, global: bool) -> bool {
        if self.include_entry_exports_forced {
            return true;
        }
        if self.keys.is_empty() {
            return global;
        }
        self.bound()
            .workspaces
            .iter()
            .find(|workspace| path.starts_with(&workspace.root))
            .and_then(|workspace| workspace.include_entry_exports)
            .unwrap_or(global)
    }

    /// Whether a key of the deepest bound workspace that contains `path`
    /// hides the dependency finding for `package_name`. Call it only for a
    /// finding that the global `ignoreDependencies` did not hide.
    #[must_use]
    pub fn suppresses_dependency_at(&self, path: &Path, package_name: &str) -> bool {
        if self.keys.is_empty() {
            return false;
        }
        let bound = self.bound();
        let Some(workspace) = bound
            .workspaces
            .iter()
            .find(|workspace| path.starts_with(&workspace.root))
        else {
            return false;
        };
        let mut suppressed = false;
        for &index in &workspace.keys {
            suppressed |= self.keys[index]
                .ignore_dependencies
                .suppresses(package_name);
        }
        suppressed
    }

    /// Forget the hits of an earlier analysis pass.
    pub fn reset_usage(&self) {
        for key in self.keys.iter() {
            key.ignore_dependencies.reset_usage();
        }
    }
}

impl super::ResolvedConfig {
    /// Start a dead-code pass: forget the pattern hits of an earlier pass and
    /// bind `workspaces.settings` to the workspaces of this pass.
    pub fn begin_dead_code_pass(&self, workspaces: &[WorkspaceInfo]) {
        self.ignore_dependencies.reset_usage();
        self.ignore_findings.reset_usage();
        self.workspace_settings.reset_usage();
        self.workspace_settings.bind(&self.root, workspaces);
    }
}

fn warn_once(message: &str) {
    static WARNED: OnceLock<Mutex<FxHashSet<String>>> = OnceLock::new();
    let warned = WARNED.get_or_init(|| Mutex::new(FxHashSet::default()));
    if warned
        .lock()
        .map_or(true, |mut set| set.insert(message.to_owned()))
    {
        tracing::warn!("{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FallowConfig;

    fn settings(entries: &[(&str, &[&str], Option<bool>)]) -> ResolvedWorkspaceSettings {
        let map = entries
            .iter()
            .map(|(key, deps, include)| {
                (
                    (*key).to_string(),
                    WorkspaceSettings {
                        ignore_dependencies: deps.iter().map(|d| (*d).to_string()).collect(),
                        include_entry_exports: *include,
                    },
                )
            })
            .collect();
        ResolvedWorkspaceSettings::compile(&map)
    }

    fn workspaces(root: &Path, roots: &[&str]) -> Vec<WorkspaceInfo> {
        roots
            .iter()
            .map(|r| WorkspaceInfo {
                root: root.join(r),
                name: (*r).to_string(),
                is_internal_dependency: false,
            })
            .collect()
    }

    #[test]
    fn unbound_settings_apply_the_global_value() {
        let s = settings(&[("packages/*", &["lodash"], Some(true))]);
        let path = Path::new("/p/packages/a/src/index.ts");
        assert!(!s.include_entry_exports_for(path, false));
        assert!(!s.suppresses_dependency_at(path, "lodash"));
    }

    #[test]
    fn glob_and_exact_keys_select_workspaces() {
        let root = Path::new("/p");
        let s = settings(&[
            ("packages/*", &["lodash"], Some(true)),
            ("packages/b", &["react"], Some(false)),
            ("apps/web", &["vite"], None),
        ]);
        s.bind(
            root,
            &workspaces(root, &["packages/a", "packages/b", "apps/web"]),
        );

        let a = Path::new("/p/packages/a/src/index.ts");
        let b = Path::new("/p/packages/b/src/index.ts");
        let web = Path::new("/p/apps/web/src/main.ts");
        assert!(s.include_entry_exports_for(a, false));
        assert!(!s.include_entry_exports_for(b, false), "exact root wins");
        assert!(s.include_entry_exports_for(web, true), "unset keeps global");

        assert!(s.suppresses_dependency_at(b, "lodash"), "lists merge");
        assert!(s.suppresses_dependency_at(b, "react"));
        assert!(!s.suppresses_dependency_at(a, "react"));
        assert!(!s.suppresses_dependency_at(web, "lodash"));
        assert!(!s.suppresses_dependency_at(Path::new("/p/src/a.ts"), "lodash"));
    }

    #[test]
    fn deepest_workspace_owns_a_file() {
        let root = Path::new("/p");
        let s = settings(&[
            ("apps/mobile", &["expo-a"], Some(true)),
            ("apps/mobile/modules/native", &[], Some(false)),
        ]);
        s.bind(
            root,
            &workspaces(root, &["apps/mobile", "apps/mobile/modules/native"]),
        );
        let nested = Path::new("/p/apps/mobile/modules/native/src/a.ts");
        assert!(!s.include_entry_exports_for(nested, false));
        assert!(!s.suppresses_dependency_at(nested, "expo-a"));
        assert!(s.include_entry_exports_for(Path::new("/p/apps/mobile/index.ts"), false));
    }

    #[test]
    fn longer_glob_wins_and_equal_globs_that_disagree_use_global() {
        let root = Path::new("/p");
        let longer = settings(&[
            ("packages/*", &[], Some(false)),
            ("packages/core/*", &[], Some(true)),
        ]);
        longer.bind(root, &workspaces(root, &["packages/core/x"]));
        assert!(longer.include_entry_exports_for(Path::new("/p/packages/core/x/a.ts"), false));

        let tie = settings(&[("packages/*", &[], Some(false)), ("*/a", &[], Some(true))]);
        tie.bind(root, &workspaces(root, &["packages/a"]));
        assert!(tie.include_entry_exports_for(Path::new("/p/packages/a/a.ts"), true));
        assert!(!tie.include_entry_exports_for(Path::new("/p/packages/a/a.ts"), false));
    }

    #[test]
    fn forced_value_wins_over_a_workspace_false() {
        let root = Path::new("/p");
        let mut s = settings(&[("packages/a", &[], Some(false))]);
        s.force_include_entry_exports();
        s.bind(root, &workspaces(root, &["packages/a"]));
        assert!(s.include_entry_exports_for(Path::new("/p/packages/a/a.ts"), true));
    }

    #[test]
    fn a_bind_on_one_clone_does_not_change_another_clone() {
        let root = Path::new("/p");
        let s = settings(&[("packages/*", &["lodash"], Some(true))]);
        let first = s.clone();
        let second = s;
        first.bind(root, &workspaces(root, &["packages/a"]));
        second.bind(
            Path::new("/other"),
            &workspaces(Path::new("/other"), &["packages/a"]),
        );

        let path = Path::new("/p/packages/a/src/index.ts");
        assert!(first.include_entry_exports_for(path, false));
        assert!(first.suppresses_dependency_at(path, "lodash"));
        assert!(!second.include_entry_exports_for(path, false));

        second.bind(root, &[]);
        assert!(
            first.include_entry_exports_for(path, false),
            "a later bind on another clone does not change this clone"
        );
    }

    #[test]
    fn keys_accept_a_leading_dot_slash_and_a_trailing_slash() {
        let root = Path::new("/p");
        let s = settings(&[("./packages/a/", &["lodash"], None)]);
        s.bind(root, &workspaces(root, &["packages/a"]));
        assert!(s.suppresses_dependency_at(Path::new("/p/packages/a/package.json"), "lodash"));
    }

    #[test]
    fn extends_merges_workspace_settings_key_by_key() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("base.json"),
            r#"{"workspaces": {"settings": {
                "packages/*": {"includeEntryExports": true, "ignoreDependencies": ["a"]},
                "apps/web": {"ignoreDependencies": ["vite"]}
            }}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": ["base.json"], "workspaces": {"settings": {
                "packages/*": {"ignoreDependencies": ["b"]}
            }}}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        let settings = config.workspaces.unwrap().settings;
        let packages = &settings["packages/*"];
        assert_eq!(packages.include_entry_exports, Some(true), "fields merge");
        assert_eq!(packages.ignore_dependencies, vec!["b"], "arrays replace");
        assert_eq!(settings["apps/web"].ignore_dependencies, vec!["vite"]);
    }

    #[test]
    fn workspaces_keeps_the_packages_alias_and_parses_settings_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("fallow.toml"),
            "supportEntry = [\"scripts/*.ts\"]\n\n[workspaces]\npackages = [\"libs/*\"]\n\n\
             [workspaces.settings.\"packages/*\"]\nincludeEntryExports = true\n\
             ignoreDependencies = [\"lodash\"]\n",
        )
        .unwrap();
        let config = FallowConfig::load(&dir.path().join("fallow.toml")).unwrap();
        assert_eq!(config.support_entry, vec!["scripts/*.ts"]);
        let workspaces = config.workspaces.unwrap();
        assert_eq!(workspaces.patterns, vec!["libs/*"]);
        assert!(workspaces.unknown_keys.is_empty());
        let packages = &workspaces.settings["packages/*"];
        assert_eq!(packages.include_entry_exports, Some(true));
        assert_eq!(packages.ignore_dependencies, vec!["lodash"]);
    }

    #[test]
    fn invalid_workspace_settings_globs_fail_to_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"workspaces": {"settings": {"packages/a": {"ignoreDependencies": ["@x/{a"]}}}}"#,
        )
        .unwrap();
        assert!(FallowConfig::load(&dir.path().join(".fallowrc.json")).is_err());

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"supportEntry": ["scripts/[a.ts"]}"#,
        )
        .unwrap();
        assert!(FallowConfig::load(&dir.path().join(".fallowrc.json")).is_err());
    }
}
