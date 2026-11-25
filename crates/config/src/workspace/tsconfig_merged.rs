//! The one reader of merged tsconfig settings.
//!
//! TypeScript reads a tsconfig together with the configs that it extends.
//! [`MergedTsconfig`] applies the same rules:
//!
//! - `extends` is one string or an array of strings. A local specifier
//!   (`./base.json`, `../tsconfig`) resolves against the folder of the config.
//!   A package specifier (`@tsconfig/node20`, `@vue/tsconfig/tsconfig.dom.json`)
//!   resolves through `node_modules`, the package `exports`, the `tsconfig`
//!   field, and the conventional `tsconfig.json`.
//! - A later entry of an array `extends` overrides an earlier entry, and the
//!   config itself overrides all of its parents.
//! - `compilerOptions` merge key by key. A key that a config sets replaces the
//!   inherited value as a whole, so `paths` is never merged pattern by pattern.
//! - `files`, `include` and `exclude` replace the inherited value as a whole.
//! - A relative path value resolves against the folder of the config that
//!   sets it, and `${configDir}` names the folder of the config that the
//!   caller reads. Each merged value keeps the folder of its config for that
//!   reason.
//!
//! The walk is bounded and cycle-safe. A parent that is missing, does not
//! parse, is part of a cycle, or is over a limit adds a [`TsconfigExtendsIssue`]
//! and contributes nothing. The other parents still apply, as in TypeScript,
//! which reports the error and keeps the rest of the chain.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    MAX_EXTENDS_DEPTH, MAX_EXTENDS_ENTRIES, MAX_EXTENDS_READS, PROJECT_BUDGET_ROOTS,
    is_local_extends, normalize_path, resolve_extends_path,
};

/// The token that TypeScript replaces with the folder of the config that it
/// reads, at the start of a path value of any config of the chain.
const CONFIG_DIR_TOKEN: &str = "${configDir}";

/// One merged value and the folder of the config that set it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsconfigValue {
    value: serde_json::Value,
    origin: PathBuf,
}

impl TsconfigValue {
    /// The JSON value as the config wrote it.
    #[must_use]
    pub const fn value(&self) -> &serde_json::Value {
        &self.value
    }

    /// The folder of the config that set the value. A relative path in the
    /// value resolves against this folder.
    #[must_use]
    pub fn origin(&self) -> &Path {
        &self.origin
    }

    /// The value as a string, or `None` when it is not a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        self.value.as_str()
    }

    /// The string items of an array value, or `None` when the value is not an
    /// array. An item that is not a string is left out.
    #[must_use]
    pub fn strings(&self) -> Option<Vec<&str>> {
        self.value
            .as_array()
            .map(|items| items.iter().filter_map(serde_json::Value::as_str).collect())
    }
}

/// A top-level file-set field of a tsconfig.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsconfigFileSet {
    /// `files`: the explicit file list.
    Files,
    /// `include`: the include patterns.
    Include,
    /// `exclude`: the exclude patterns.
    Exclude,
}

impl TsconfigFileSet {
    const fn key(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Include => "include",
            Self::Exclude => "exclude",
        }
    }
}

/// A problem in the `extends` chain. The config that has the problem
/// contributes nothing to the merged settings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TsconfigExtendsIssue {
    /// An `extends` specifier of `config` does not resolve to a file.
    Missing {
        /// The config that holds the specifier.
        config: PathBuf,
        /// The specifier as the config wrote it.
        specifier: String,
    },
    /// A parent config exists but does not read or parse.
    Malformed(PathBuf),
    /// `extends` of this config is not a string or an array of strings.
    InvalidExtends(PathBuf),
    /// `compilerOptions` of this config is not an object.
    InvalidCompilerOptions(PathBuf),
    /// This parent config is already on the current `extends` path.
    Cycle(PathBuf),
    /// The walk reached this config over the depth limit or a read limit.
    Limit(PathBuf),
}

/// A function that returns the parsed content of one config file, or `None`
/// when the file does not read or parse.
pub type SourceLoader<'a> = dyn Fn(&Path) -> Option<Arc<serde_json::Value>> + 'a;

/// One `include` or `exclude` pattern and the folder that it is relative to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsconfigPattern {
    /// The folder of the config that sets the pattern, or the folder of the
    /// config that the caller reads for a pattern that starts with
    /// `${configDir}`.
    pub base: PathBuf,
    /// The pattern relative to `base`.
    pub pattern: String,
}

/// The settings of a tsconfig after TypeScript applies its `extends` chain.
#[derive(Debug, Clone)]
pub struct MergedTsconfig {
    config_dir: PathBuf,
    merged: Arc<Merged>,
}

/// The `paths` option and the folders that its targets resolve against.
#[derive(Debug, Clone)]
pub struct TsconfigPaths<'a> {
    /// The `paths` object: pattern to target list.
    pub patterns: &'a serde_json::Map<String, serde_json::Value>,
    /// `baseUrl` when a config of the chain sets it, else the folder of the
    /// config that sets `paths`. TypeScript 5 resolves targets against it.
    pub base_dir: PathBuf,
    /// The folder of the config that sets `paths`. TypeScript 7 has no
    /// `baseUrl` and resolves targets against this folder.
    pub origin_dir: PathBuf,
}

impl TsconfigPaths<'_> {
    /// The folders to resolve a target against, in lookup order:
    /// [`Self::base_dir`], then [`Self::origin_dir`] when it differs. A
    /// reader that checks that the target exists tries both, so a project
    /// on either TypeScript version keeps its aliases.
    pub fn base_dirs(&self) -> impl Iterator<Item = &Path> {
        let origin = (self.origin_dir != self.base_dir).then_some(self.origin_dir.as_path());
        std::iter::once(self.base_dir.as_path()).chain(origin)
    }
}

impl MergedTsconfig {
    /// Read the config at `config_path` and its `extends` chain.
    ///
    /// Returns `None` when `config_path` does not read or parse. A problem in
    /// a parent is in [`Self::issues`].
    #[must_use]
    pub fn read(config_path: &Path) -> Option<Self> {
        ExtendsWalk::new().read(config_path, None)
    }

    /// Read the `extends` chain of a config whose content the caller has
    /// already loaded. The walk reads only the parents from disk.
    ///
    /// Returns `None` when `source` does not parse.
    #[must_use]
    pub fn read_with_source(config_path: &Path, source: &str) -> Option<Self> {
        let value = parse_source(source)?;
        ExtendsWalk::new().read(config_path, Some(Arc::new(value)))
    }

    /// Read a config and its `extends` chain through `load`, which returns
    /// the parsed content of one config file. A caller with its own cache of
    /// parsed configs passes it here, so the configs that many leaves share
    /// are parsed once.
    ///
    /// Returns `None` when `load` gives no content for `config_path`.
    #[must_use]
    pub fn read_with_loader(config_path: &Path, load: &SourceLoader<'_>) -> Option<Self> {
        let mut walk = ExtendsWalk::new();
        walk.loader = Some(load);
        walk.read(config_path, None)
    }

    /// The folder of the config that the caller read. `${configDir}` names
    /// this folder.
    #[must_use]
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// The merged value of one `compilerOptions` key.
    #[must_use]
    pub fn compiler_option(&self, name: &str) -> Option<&TsconfigValue> {
        self.merged.compiler_options.get(name)
    }

    /// The merged value of one `compilerOptions` key, when it is a string.
    #[must_use]
    pub fn compiler_option_str(&self, name: &str) -> Option<&str> {
        self.compiler_option(name)?.as_str()
    }

    /// The merged value of `files`, `include` or `exclude`.
    #[must_use]
    pub fn file_set(&self, field: TsconfigFileSet) -> Option<&TsconfigValue> {
        self.merged.file_sets.get(field.key())
    }

    /// Resolve a path value that a config with folder `origin` wrote:
    /// replace `${configDir}`, keep an absolute path, and join a relative
    /// path to `origin`. Returns `None` when the value is empty or climbs
    /// above the file system root.
    #[must_use]
    pub fn resolve_path(&self, value: &str, origin: &Path) -> Option<PathBuf> {
        if value.is_empty() {
            return None;
        }
        let (base, value) = self.split_config_dir(value, origin);
        let path = Path::new(value);
        if path.is_absolute() {
            normalize_path(path)
        } else {
            normalize_path(&base.join(path))
        }
    }

    /// The folder that `value` is relative to, and the rest of `value`. A
    /// value that starts with `${configDir}` is relative to the folder of
    /// the config that the caller reads.
    fn split_config_dir<'v>(&'v self, value: &'v str, origin: &'v Path) -> (&'v Path, &'v str) {
        match value.strip_prefix(CONFIG_DIR_TOKEN) {
            Some(rest) => (&self.config_dir, rest.trim_start_matches(['/', '\\'])),
            None => (origin, value),
        }
    }

    /// The string patterns of the merged `include` or `exclude`, each with
    /// the folder that it is relative to. A pattern that is not a string is
    /// left out. `None` when no config sets the field, or when it is not an
    /// array.
    #[must_use]
    pub fn file_set_patterns(&self, field: TsconfigFileSet) -> Option<Vec<TsconfigPattern>> {
        let value = self.file_set(field)?;
        Some(
            value
                .strings()?
                .into_iter()
                .map(|pattern| {
                    let (base, pattern) = self.split_config_dir(pattern, value.origin());
                    TsconfigPattern {
                        base: base.to_path_buf(),
                        pattern: pattern.to_owned(),
                    }
                })
                .collect(),
        )
    }

    /// The resolved path of a string option such as `rootDir`, `outDir` or
    /// `baseUrl`. `None` when no config sets it, or when it is not a usable
    /// path string.
    #[must_use]
    pub fn path_option(&self, name: &str) -> Option<PathBuf> {
        let option = self.compiler_option(name)?;
        self.resolve_path(option.as_str()?, option.origin())
    }

    /// The resolved paths of a string-array option such as `typeRoots` or
    /// `rootDirs`. An item that is not a usable path string is left out.
    #[must_use]
    pub fn path_list_option(&self, name: &str) -> Option<Vec<PathBuf>> {
        let option = self.compiler_option(name)?;
        Some(
            option
                .strings()?
                .into_iter()
                .filter_map(|item| self.resolve_path(item, option.origin()))
                .collect(),
        )
    }

    /// The merged `paths` option with the folder that its targets resolve
    /// against, as TypeScript resolves them.
    #[must_use]
    pub fn paths(&self) -> Option<TsconfigPaths<'_>> {
        let paths = self.compiler_option("paths")?;
        let patterns = paths.value().as_object()?;
        let origin_dir = paths.origin().to_path_buf();
        let base_dir = self
            .path_option("baseUrl")
            .unwrap_or_else(|| origin_dir.clone());
        Some(TsconfigPaths {
            patterns,
            base_dir,
            origin_dir,
        })
    }

    /// The problems of the `extends` chain, in walk order.
    #[must_use]
    pub fn issues(&self) -> &[TsconfigExtendsIssue] {
        &self.merged.issues
    }

    /// True when every config of the chain resolved, parsed and fit the
    /// limits.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.merged.issues.is_empty()
    }

    /// True when a parent config exists but does not parse.
    #[must_use]
    pub fn has_malformed_parent(&self) -> bool {
        self.merged
            .issues
            .iter()
            .any(|issue| matches!(issue, TsconfigExtendsIssue::Malformed(_)))
    }
}

/// The merged settings of one config and its chain.
#[derive(Debug, Clone, Default)]
struct Merged {
    compiler_options: FxHashMap<String, TsconfigValue>,
    file_sets: FxHashMap<&'static str, TsconfigValue>,
    issues: Vec<TsconfigExtendsIssue>,
    seen_issues: FxHashSet<TsconfigExtendsIssue>,
}

impl Merged {
    fn with_issue(issue: TsconfigExtendsIssue) -> Self {
        let mut merged = Self::default();
        merged.push_issue(issue);
        merged
    }

    /// Apply the settings of `parent` over the current settings.
    fn inherit(&mut self, parent: &Self) {
        for (name, value) in &parent.compiler_options {
            self.compiler_options.insert(name.clone(), value.clone());
        }
        for (name, value) in &parent.file_sets {
            self.file_sets.insert(name, value.clone());
        }
        for issue in &parent.issues {
            self.push_issue(issue.clone());
        }
    }

    /// Add an issue once. Two parents can share an ancestor with a problem,
    /// and without this check a lattice of shared parents would copy the
    /// issue once for each path through the lattice.
    fn push_issue(&mut self, issue: TsconfigExtendsIssue) {
        if self.seen_issues.insert(issue.clone()) {
            self.issues.push(issue);
        }
    }

    /// A result that depends on the current walk path or on a spent limit.
    /// The walk must not reuse it for another path.
    fn is_transient(&self) -> bool {
        self.issues.iter().any(|issue| {
            matches!(
                issue,
                TsconfigExtendsIssue::Cycle(_) | TsconfigExtendsIssue::Limit(_)
            )
        })
    }
}

fn parse_source(source: &str) -> Option<serde_json::Value> {
    crate::jsonc::parse_to_value(source.trim_start_matches('\u{FEFF}')).ok()
}

/// The `extends` walk of one or more root configs.
///
/// The walk reads and parses each config once, resolves each `extends`
/// specifier of a config folder once, and keeps the merged settings of each
/// finished config. It stops at [`MAX_EXTENDS_READS`] config expansions and
/// [`MAX_EXTENDS_ENTRIES`] `extends` entries for each root config, and all
/// root configs of one walk together stop at [`PROJECT_BUDGET_ROOTS`] of
/// these budgets. A config over a limit adds a
/// [`TsconfigExtendsIssue::Limit`], so an untrusted repository cannot make the
/// walk read an unbounded number of files.
pub(super) struct ExtendsWalk<'a> {
    /// The canonical configs on the current `extends` path, to detect a cycle.
    in_progress: FxHashSet<PathBuf>,
    /// The merged settings of each finished config, with the height of its
    /// `extends` tree.
    resolved: FxHashMap<PathBuf, (Arc<Merged>, usize)>,
    /// The target of each `extends` specifier, by config folder.
    targets: FxHashMap<PathBuf, FxHashMap<String, Option<PathBuf>>>,
    /// The parsed content of each config file, or `None` when the file does
    /// not read or parse.
    sources: FxHashMap<PathBuf, Option<Arc<serde_json::Value>>>,
    /// The loader of config content. `None` reads and parses each file.
    loader: Option<&'a SourceLoader<'a>>,
    /// Resolve a package parent to its real path, as TypeScript module
    /// resolution does through the symlinks of `node_modules`.
    real_package_paths: bool,
    /// The config expansions of the current root config and of the walk.
    reads: WorkBudget,
    /// The `extends` entries of the current root config and of the walk.
    entries: WorkBudget,
}

/// A work count with a limit for one root config and a limit for the walk.
struct WorkBudget {
    root_limit: usize,
    root: usize,
    project: usize,
}

impl WorkBudget {
    const fn new(root_limit: usize) -> Self {
        Self {
            root_limit,
            root: 0,
            project: 0,
        }
    }

    fn start_root(&mut self) {
        self.root = 0;
    }

    fn project_spent(&self) -> bool {
        self.project >= self.root_limit * PROJECT_BUDGET_ROOTS
    }

    /// Count one unit of work. `false` when a limit is spent.
    fn take(&mut self) -> bool {
        if self.root >= self.root_limit || self.project_spent() {
            return false;
        }
        self.root += 1;
        self.project += 1;
        true
    }
}

impl ExtendsWalk<'_> {
    pub(super) fn new() -> Self {
        Self {
            in_progress: FxHashSet::default(),
            resolved: FxHashMap::default(),
            targets: FxHashMap::default(),
            sources: FxHashMap::default(),
            loader: None,
            real_package_paths: false,
            reads: WorkBudget::new(MAX_EXTENDS_READS),
            entries: WorkBudget::new(MAX_EXTENDS_ENTRIES),
        }
    }

    /// A walk that resolves a package parent to its real path. A caller
    /// whose root is a real path, such as [`super::TsconfigOutputMap`], gets
    /// the folder of origin that TypeScript uses for a package config.
    pub(super) fn with_real_package_paths() -> Self {
        Self {
            real_package_paths: true,
            ..Self::new()
        }
    }

    /// Read one root config. `leaf_source` is its parsed content when the
    /// caller has it. Returns `None` when the root config does not read or
    /// parse, or when the walk has spent the budget of all its root configs.
    pub(super) fn read(
        &mut self,
        config_path: &Path,
        leaf_source: Option<Arc<serde_json::Value>>,
    ) -> Option<MergedTsconfig> {
        self.in_progress.clear();
        self.reads.start_root();
        self.entries.start_root();
        if self.reads.project_spent() || self.entries.project_spent() {
            tracing::debug!(
                config = %config_path.display(),
                "tsconfig extends walk spent the budget of the project; ignoring the config"
            );
            return None;
        }
        let config_path = normalize_path(config_path).unwrap_or_else(|| config_path.to_path_buf());
        let config_dir = config_path.parent()?.to_path_buf();
        if let Some(source) = leaf_source {
            self.sources.insert(config_path.clone(), Some(source));
        } else {
            self.source(&config_path)?;
        }
        let (merged, _) = self.merge(&config_path, 0);
        Some(MergedTsconfig { config_dir, merged })
    }

    /// The merged settings of `config_path` and the height of its `extends`
    /// tree. A config without `extends` has height 1.
    fn merge(&mut self, config_path: &Path, depth: usize) -> (Arc<Merged>, usize) {
        let issue = |issue: fn(PathBuf) -> TsconfigExtendsIssue| {
            (
                Arc::new(Merged::with_issue(issue(config_path.to_path_buf()))),
                0,
            )
        };
        if depth >= MAX_EXTENDS_DEPTH {
            return issue(TsconfigExtendsIssue::Limit);
        }
        if let Some((merged, height)) = self.resolved.get(config_path) {
            // A deeper path to a finished config can still exceed the depth limit.
            if depth + height <= MAX_EXTENDS_DEPTH {
                return (Arc::clone(merged), *height);
            }
            return issue(TsconfigExtendsIssue::Limit);
        }
        let identity =
            dunce::canonicalize(config_path).unwrap_or_else(|_| config_path.to_path_buf());
        if !self.in_progress.insert(identity.clone()) {
            return issue(TsconfigExtendsIssue::Cycle);
        }
        if !self.reads.take() {
            self.in_progress.remove(&identity);
            tracing::debug!(
                config = %config_path.display(),
                limit = MAX_EXTENDS_READS,
                "tsconfig extends walk reads too many configs; ignoring the rest of the chain"
            );
            return issue(TsconfigExtendsIssue::Limit);
        }
        let (merged, height) = self.parse(config_path, depth);
        self.in_progress.remove(&identity);
        let merged = Arc::new(merged);
        if !merged.is_transient() {
            self.resolved
                .insert(config_path.to_path_buf(), (Arc::clone(&merged), height));
        }
        (merged, height)
    }

    fn parse(&mut self, config_path: &Path, depth: usize) -> (Merged, usize) {
        let Some(value) = self.source(config_path) else {
            return (
                Merged::with_issue(TsconfigExtendsIssue::Malformed(config_path.to_path_buf())),
                0,
            );
        };
        let mut merged = Merged::default();
        let specifiers = match value.get("extends") {
            Some(serde_json::Value::String(extends)) => vec![extends.as_str()],
            Some(serde_json::Value::Array(items)) => {
                if items.iter().any(|item| !item.is_string()) {
                    merged.push_issue(TsconfigExtendsIssue::InvalidExtends(
                        config_path.to_path_buf(),
                    ));
                }
                items.iter().filter_map(serde_json::Value::as_str).collect()
            }
            Some(_) => {
                merged.push_issue(TsconfigExtendsIssue::InvalidExtends(
                    config_path.to_path_buf(),
                ));
                Vec::new()
            }
            None => Vec::new(),
        };
        let parents = self.resolve_parents(config_path, &specifiers, &mut merged);
        let mut parent_height = 0;
        for parent in parents {
            let (parent_merged, height) = self.merge(&parent, depth + 1);
            parent_height = parent_height.max(height);
            merged.inherit(&parent_merged);
        }
        let Some(config_dir) = config_path.parent() else {
            return (merged, parent_height + 1);
        };
        match value.get("compilerOptions") {
            Some(serde_json::Value::Object(options)) => {
                for (name, option) in options {
                    merged.compiler_options.insert(
                        name.clone(),
                        TsconfigValue {
                            value: option.clone(),
                            origin: config_dir.to_path_buf(),
                        },
                    );
                }
            }
            Some(_) => merged.push_issue(TsconfigExtendsIssue::InvalidCompilerOptions(
                config_path.to_path_buf(),
            )),
            None => {}
        }
        for field in [
            TsconfigFileSet::Files,
            TsconfigFileSet::Include,
            TsconfigFileSet::Exclude,
        ] {
            if let Some(file_set) = value.get(field.key()) {
                merged.file_sets.insert(
                    field.key(),
                    TsconfigValue {
                        value: file_set.clone(),
                        origin: config_dir.to_path_buf(),
                    },
                );
            }
        }
        (merged, parent_height + 1)
    }

    /// The parent configs of one `extends` value, in apply order. A repeated
    /// parent keeps only its last place, because the last place decides
    /// which settings it overrides.
    fn resolve_parents(
        &mut self,
        config_path: &Path,
        specifiers: &[&str],
        merged: &mut Merged,
    ) -> Vec<PathBuf> {
        let Some(config_dir) = config_path.parent() else {
            return Vec::new();
        };
        let mut seen_specifiers = FxHashSet::default();
        let mut seen_parents = FxHashSet::default();
        let mut parents = Vec::new();
        for &specifier in specifiers.iter().rev() {
            if !seen_specifiers.insert(specifier) {
                continue;
            }
            if !self.entries.take() {
                tracing::debug!(
                    config = %config_path.display(),
                    limit = MAX_EXTENDS_ENTRIES,
                    "tsconfig extends walk follows too many entries; ignoring the rest of the chain"
                );
                merged.push_issue(TsconfigExtendsIssue::Limit(config_path.to_path_buf()));
                break;
            }
            match self.resolve_target(config_path, config_dir, specifier) {
                Some(parent) => {
                    if seen_parents.insert(parent.clone()) {
                        parents.push(parent);
                    }
                }
                None => merged.push_issue(TsconfigExtendsIssue::Missing {
                    config: config_path.to_path_buf(),
                    specifier: specifier.to_owned(),
                }),
            }
        }
        parents.reverse();
        parents
    }

    /// The config file of one specifier. The target keeps its joined path, so
    /// the folder of origin of each value has the same spelling as the
    /// folder of the config that the caller reads, unless the walk resolves
    /// package parents to their real path. The cycle check compares real
    /// paths.
    fn resolve_target(
        &mut self,
        config_path: &Path,
        config_dir: &Path,
        specifier: &str,
    ) -> Option<PathBuf> {
        if let Some(target) = self
            .targets
            .get(config_dir)
            .and_then(|targets| targets.get(specifier))
        {
            return target.clone();
        }
        let target = resolve_extends_path(config_path, specifier).and_then(|path| {
            if self.real_package_paths && !is_local_extends(specifier) {
                dunce::canonicalize(path).ok()
            } else {
                Some(path)
            }
        });
        self.targets
            .entry(config_dir.to_path_buf())
            .or_default()
            .insert(specifier.to_owned(), target.clone());
        target
    }

    /// The parsed content of `config_path`, read once per walk.
    fn source(&mut self, config_path: &Path) -> Option<Arc<serde_json::Value>> {
        if let Some(source) = self.sources.get(config_path) {
            return source.clone();
        }
        let source = match self.loader {
            Some(load) => load(config_path),
            None => fallow_types::read_guard::read_to_string_bounded(
                config_path,
                fallow_types::read_guard::DEFAULT_MAX_READ_BYTES,
            )
            .ok()
            .and_then(|content| parse_source(&content))
            .map(Arc::new),
        };
        self.sources
            .insert(config_path.to_path_buf(), source.clone());
        source
    }
}

#[cfg(test)]
#[path = "tsconfig_merged_tests.rs"]
mod tests;
