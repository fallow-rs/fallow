use std::path::{Component, Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

const MAX_EXTENDS_DEPTH: usize = 16;
/// The most distinct config files that the `extends` walk of one root config
/// reads. A walk over this limit fails closed, so an untrusted repository can
/// not make the walk read an unbounded number of files.
const MAX_EXTENDS_READS: usize = 256;
/// The most `extends` entries that the walk of one root config follows,
/// after it drops repeated specifiers of one array. A walk over this limit
/// fails closed.
const MAX_EXTENDS_ENTRIES: usize = 4096;
const DECLARATION_OUTPUT_SUFFIXES: &[&str] = &[".d.ts", ".d.mts", ".d.cts"];

#[derive(Debug, Clone, Default)]
struct CompilerOptions {
    root_dir: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    declaration_dir: Option<PathBuf>,
    no_emit: Option<bool>,
    emit_declaration_only: Option<bool>,
    /// The folder of the leaf config. TypeScript 6 uses it as the default
    /// `rootDir`.
    config_dir: Option<PathBuf>,
}

/// How a mapping treats a config without an explicit `rootDir`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootDirDefault {
    /// Skip the config. A package entry keeps its other fallbacks.
    Skip,
    /// Use the folder of the config, as TypeScript 6 does. The config then
    /// maps an existing source file, but does not claim the output.
    ConfigDir,
}

/// Outcome for a package entry that may point into a configured TypeScript output directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TsconfigOutputResolution {
    /// No supported config declares this entry as an output.
    Unconfigured,
    /// A config declares the output path, but the configs map it to two or
    /// more source files.
    ConfiguredButUnresolved,
    /// A config declares the output path, but no config maps it to an
    /// existing source file. A `noEmit` config maps nothing. Another build
    /// tool can own the output, so a caller may try its own source mapping.
    ConfiguredWithoutSource,
    /// The configured output maps to one source file inside the project.
    Resolved(PathBuf),
}

/// Static package output mappings read from a package's TypeScript configs.
#[derive(Debug, Clone, Default)]
pub struct TsconfigOutputMap {
    root: Option<PathBuf>,
    display_root: Option<PathBuf>,
    configs: Vec<CompilerOptions>,
}

impl TsconfigOutputMap {
    /// Read root-level tsconfig files once, resolving static relative and package `extends` paths.
    #[must_use]
    pub fn from_project(root: &Path) -> Self {
        let display_root = root.to_path_buf();
        let Some(canonical_root) = dunce::canonicalize(root).ok() else {
            return Self::default();
        };
        let Some(config_paths) = tsconfig_paths(&canonical_root) else {
            return Self::default();
        };
        // Every root config lives in `canonical_root`, so the walk can share
        // its memo across them.
        let mut walk = ExtendsWalk::new(&canonical_root);
        let configs = config_paths
            .iter()
            .filter_map(|path| {
                let config_dir = path.parent()?;
                let mut options = walk.read(path)?;
                options.config_dir = Some(config_dir.to_path_buf());
                Some(options)
            })
            .collect();
        Self {
            root: Some(canonical_root),
            display_root: Some(display_root),
            configs,
        }
    }

    /// Resolve an absolute output path, such as the target of a relative import,
    /// to its source file.
    ///
    /// The path does not need to exist on disk. A path outside the project root
    /// of this map is [`TsconfigOutputResolution::Unconfigured`].
    #[must_use]
    pub fn resolve_source_for_output_path(
        &self,
        output_path: &Path,
        source_extensions: &[&str],
    ) -> TsconfigOutputResolution {
        let Some(output_path) = normalize_path(output_path) else {
            return TsconfigOutputResolution::Unconfigured;
        };
        let relative = [self.display_root.as_deref(), self.root.as_deref()]
            .into_iter()
            .flatten()
            .find_map(|root| output_path.strip_prefix(root).ok());
        let Some(entry) = relative.and_then(Path::to_str) else {
            return TsconfigOutputResolution::Unconfigured;
        };
        self.resolve_source(entry, source_extensions, RootDirDefault::ConfigDir)
    }

    /// Resolve a generated entry while preserving whether the output was configured.
    #[must_use]
    pub fn resolve_source_for_entry(
        &self,
        entry: &str,
        source_extensions: &[&str],
    ) -> TsconfigOutputResolution {
        self.resolve_source(entry, source_extensions, RootDirDefault::Skip)
    }

    fn resolve_source(
        &self,
        entry: &str,
        source_extensions: &[&str],
        root_dir_default: RootDirDefault,
    ) -> TsconfigOutputResolution {
        let Some(root) = self.root.as_deref() else {
            return TsconfigOutputResolution::Unconfigured;
        };
        let Some(output) = OutputEntry::new(root, entry) else {
            return TsconfigOutputResolution::Unconfigured;
        };
        let mut sources = SourceCandidates::default();
        for options in &self.configs {
            sources.add_config(options, &output, root, source_extensions, root_dir_default);
        }
        sources.resolution(root, self.display_root.as_deref())
    }
}

/// An output path inside the project root, such as `dist/index.js`.
struct OutputEntry {
    path: PathBuf,
    is_declaration: bool,
}

impl OutputEntry {
    /// `None` for an absolute entry, an entry that leaves the root, or an
    /// entry without a UTF-8 file name.
    fn new(root: &Path, entry: &str) -> Option<Self> {
        let entry_path = Path::new(entry);
        if entry_path.is_absolute()
            || entry_path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return None;
        }
        let path = normalize_path(&root.join(entry_path))?;
        let file_name = path.file_name()?.to_str()?;
        let is_declaration = DECLARATION_OUTPUT_SUFFIXES
            .iter()
            .any(|suffix| file_name.ends_with(suffix));
        Some(Self {
            path,
            is_declaration,
        })
    }
}

impl CompilerOptions {
    /// The folder where this config emits `output`.
    fn output_dir(&self, output: &OutputEntry) -> Option<&Path> {
        if output.is_declaration {
            self.declaration_dir.as_deref().or(self.out_dir.as_deref())
        } else {
            self.out_dir.as_deref()
        }
    }

    /// Whether this config emits `output` itself. With `emitDeclarationOnly`,
    /// another tool (swc, Babel, a bundler) can emit the JavaScript into
    /// `outDir` with the same layout. A source file with the same stem then
    /// maps, but the config does not claim the output, so the callers keep
    /// their other fallbacks.
    fn claims_output(&self, output: &OutputEntry) -> bool {
        output.is_declaration || !self.emit_declaration_only.unwrap_or(false)
    }
}

/// The source files that the configs map one output path to.
///
/// Candidates of a config with an explicit `rootDir` win over the candidates
/// of a config folder default, so an inferred `rootDir` can not make an
/// explicit mapping ambiguous.
#[derive(Default)]
struct SourceCandidates {
    explicit: FxHashSet<PathBuf>,
    inferred: FxHashSet<PathBuf>,
    /// A config with an explicit `rootDir` claims the output path.
    configured: bool,
}

impl SourceCandidates {
    fn add_config(
        &mut self,
        options: &CompilerOptions,
        output: &OutputEntry,
        root: &Path,
        source_extensions: &[&str],
        root_dir_default: RootDirDefault,
    ) {
        let Some(output_dir) = options.output_dir(output) else {
            return;
        };
        let Some((source_stem, family_extensions)) =
            source_suffix_for_output(&output.path, output_dir)
        else {
            return;
        };
        let (root_dir, target) = match (options.root_dir.as_deref(), root_dir_default) {
            (Some(root_dir), _) => {
                self.configured |= options.claims_output(output);
                (root_dir, &mut self.explicit)
            }
            (None, RootDirDefault::ConfigDir) => match options.config_dir.as_deref() {
                Some(config_dir) => (config_dir, &mut self.inferred),
                None => return,
            },
            (None, RootDirDefault::Skip) => return,
        };
        if options.no_emit.unwrap_or(false) {
            return;
        }
        let sources = family_extensions
            .iter()
            .filter(|extension| source_extensions.contains(extension))
            .filter_map(|extension| existing_source(root, root_dir, &source_stem, extension));
        target.extend(sources);
    }

    fn resolution(self, root: &Path, display_root: Option<&Path>) -> TsconfigOutputResolution {
        let configured = self.configured;
        let candidates = if self.explicit.is_empty() {
            self.inferred
        } else {
            self.explicit
        };
        if candidates.is_empty() && configured {
            return TsconfigOutputResolution::ConfiguredWithoutSource;
        }
        if candidates.len() != 1 {
            return if configured {
                TsconfigOutputResolution::ConfiguredButUnresolved
            } else {
                TsconfigOutputResolution::Unconfigured
            };
        }
        candidates
            .into_iter()
            .next()
            .and_then(|source| Some(display_root?.join(source.strip_prefix(root).ok()?)))
            .map_or(
                TsconfigOutputResolution::ConfiguredButUnresolved,
                TsconfigOutputResolution::Resolved,
            )
    }
}

/// The canonical source file `root_dir/source_stem.extension` when it exists
/// inside `root`.
fn existing_source(
    root: &Path,
    root_dir: &Path,
    source_stem: &Path,
    extension: &str,
) -> Option<PathBuf> {
    let candidate = root_dir.join(append_extension(source_stem, extension)?);
    let candidate = dunce::canonicalize(candidate).ok()?;
    (candidate.starts_with(root) && candidate.is_file()).then_some(candidate)
}

fn tsconfig_paths(root: &Path) -> Option<Vec<PathBuf>> {
    let mut configs = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("tsconfig"))
                && path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("json")
                        || extension.eq_ignore_ascii_case("jsonc")
                })
        })
        .collect::<Vec<_>>();
    configs.sort();
    Some(configs)
}

/// The `extends` walk over the root configs of one project.
///
/// All root configs share one memo, because they share one
/// `substitution_base`. The walk reads and parses each config once, resolves
/// each `extends` specifier of a config folder once, and stops at
/// [`MAX_EXTENDS_READS`] new configs and [`MAX_EXTENDS_ENTRIES`] `extends`
/// entries for each root config. Over a limit, only the current root config
/// fails closed. The walk does not keep a failure that comes from a limit, so
/// a later root config can still resolve the configs that it shares. The
/// total cost stays linear: each root config does at most one budget of work.
struct ExtendsWalk<'a> {
    substitution_base: &'a Path,
    /// The configs on the current `extends` path, to detect a cycle.
    in_progress: FxHashSet<PathBuf>,
    /// The merged options of each finished config, with the height of its
    /// `extends` tree.
    resolved: FxHashMap<PathBuf, (CompilerOptions, usize)>,
    /// The lowest depth at which each failed config failed. A failure is the
    /// same or worse at a greater depth.
    failed: FxHashMap<PathBuf, usize>,
    /// The canonical target of each `extends` specifier, by config folder.
    targets: FxHashMap<PathBuf, FxHashMap<String, Option<PathBuf>>>,
    /// The reads and `extends` entries of the current root config.
    reads: usize,
    entries: usize,
    /// The current root config went over a limit.
    over_limit: bool,
}

impl<'a> ExtendsWalk<'a> {
    fn new(substitution_base: &'a Path) -> Self {
        Self {
            substitution_base,
            in_progress: FxHashSet::default(),
            resolved: FxHashMap::default(),
            failed: FxHashMap::default(),
            targets: FxHashMap::default(),
            reads: 0,
            entries: 0,
            over_limit: false,
        }
    }

    /// Read the merged compiler options of a root config and its `extends`
    /// chain. `None` when a config in the chain is missing, invalid, or part
    /// of a cycle, when the chain is deeper than [`MAX_EXTENDS_DEPTH`], or
    /// when the walk is over one of its limits.
    fn read(&mut self, config_path: &Path) -> Option<CompilerOptions> {
        self.in_progress.clear();
        self.reads = 0;
        self.entries = 0;
        self.over_limit = false;
        let config_path = dunce::canonicalize(config_path).ok()?;
        self.read_with_height(&config_path, 0)
            .map(|(options, _)| options)
    }

    /// The merged options of the canonical `config_path` and the height of
    /// its `extends` tree. A config without `extends` has height 1.
    fn read_with_height(
        &mut self,
        config_path: &Path,
        depth: usize,
    ) -> Option<(CompilerOptions, usize)> {
        if depth >= MAX_EXTENDS_DEPTH {
            return None;
        }
        if let Some((options, height)) = self.resolved.get(config_path) {
            // A deeper path to a finished config can still exceed the depth limit.
            return (depth + height <= MAX_EXTENDS_DEPTH).then(|| (options.clone(), *height));
        }
        if self
            .failed
            .get(config_path)
            .is_some_and(|&failed_depth| failed_depth <= depth)
        {
            return None;
        }
        if !self.in_progress.insert(config_path.to_path_buf()) {
            return None;
        }
        let result = if self.reads >= MAX_EXTENDS_READS {
            tracing::debug!(
                config = %config_path.display(),
                limit = MAX_EXTENDS_READS,
                "tsconfig extends walk reads too many configs; ignoring the output mapping"
            );
            self.over_limit = true;
            None
        } else {
            self.reads += 1;
            self.parse(config_path, depth)
        };
        self.in_progress.remove(config_path);
        match &result {
            Some((options, height)) => {
                self.resolved
                    .insert(config_path.to_path_buf(), (options.clone(), *height));
            }
            // A failure after a limit can come from the limit and not from
            // the config, so a later root config must walk it again.
            None if self.over_limit => {}
            None => {
                let failed_depth = self
                    .failed
                    .entry(config_path.to_path_buf())
                    .or_insert(depth);
                *failed_depth = (*failed_depth).min(depth);
            }
        }
        result
    }

    /// The canonical parent configs of one `extends` value, in apply order.
    /// A repeated parent keeps only its last place, because the last place
    /// decides which options it overrides.
    fn resolve_parents(&mut self, config_path: &Path, extends: &[&str]) -> Option<Vec<PathBuf>> {
        let config_dir = config_path.parent()?;
        let mut seen_specifiers = FxHashSet::default();
        let mut seen_parents = FxHashSet::default();
        let mut parents = Vec::new();
        for &specifier in extends.iter().rev() {
            if !seen_specifiers.insert(specifier) {
                continue;
            }
            if self.entries >= MAX_EXTENDS_ENTRIES {
                tracing::debug!(
                    config = %config_path.display(),
                    limit = MAX_EXTENDS_ENTRIES,
                    "tsconfig extends walk follows too many entries; ignoring the output mapping"
                );
                self.over_limit = true;
                return None;
            }
            self.entries += 1;
            let parent = self.resolve_target(config_path, config_dir, specifier)?;
            if seen_parents.insert(parent.clone()) {
                parents.push(parent);
            }
        }
        parents.reverse();
        Some(parents)
    }

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
        let target = resolve_extends_path(config_path, specifier)
            .and_then(|path| dunce::canonicalize(path).ok());
        self.targets
            .entry(config_dir.to_path_buf())
            .or_default()
            .insert(specifier.to_owned(), target.clone());
        target
    }

    fn parse(&mut self, config_path: &Path, depth: usize) -> Option<(CompilerOptions, usize)> {
        let substitution_base = self.substitution_base;
        let content = std::fs::read_to_string(config_path).ok()?;
        let value: serde_json::Value =
            crate::jsonc::parse_to_value(content.trim_start_matches('\u{FEFF}')).ok()?;
        let extends: Vec<&str> = match value.get("extends") {
            Some(serde_json::Value::String(extends)) => vec![extends.as_str()],
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .map(serde_json::Value::as_str)
                .collect::<Option<_>>()?,
            Some(_) => return None,
            None => Vec::new(),
        };
        let parents = self.resolve_parents(config_path, &extends)?;
        // A later entry of an array `extends` overrides an earlier one.
        let mut options = CompilerOptions::default();
        let mut parent_height = 0;
        for parent in parents {
            let (parent_options, height) = self.read_with_height(&parent, depth + 1)?;
            parent_height = parent_height.max(height);
            options.inherit(parent_options);
        }
        let height = parent_height + 1;
        let Some(compiler_options) = value.get("compilerOptions") else {
            return Some((options, height));
        };
        let compiler_options = compiler_options.as_object()?;
        let config_dir = config_path.parent()?;

        replace_path_option(
            compiler_options,
            "rootDir",
            config_dir,
            substitution_base,
            &mut options.root_dir,
        )?;
        replace_path_option(
            compiler_options,
            "outDir",
            config_dir,
            substitution_base,
            &mut options.out_dir,
        )?;
        replace_path_option(
            compiler_options,
            "declarationDir",
            config_dir,
            substitution_base,
            &mut options.declaration_dir,
        )?;
        replace_bool_option(compiler_options, "noEmit", &mut options.no_emit)?;
        replace_bool_option(
            compiler_options,
            "emitDeclarationOnly",
            &mut options.emit_declaration_only,
        )?;
        Some((options, height))
    }
}

impl CompilerOptions {
    /// Apply the options that `parent` sets over the current options.
    fn inherit(&mut self, parent: Self) {
        self.root_dir = parent.root_dir.or_else(|| self.root_dir.take());
        self.out_dir = parent.out_dir.or_else(|| self.out_dir.take());
        self.declaration_dir = parent
            .declaration_dir
            .or_else(|| self.declaration_dir.take());
        self.no_emit = parent.no_emit.or(self.no_emit);
        self.emit_declaration_only = parent.emit_declaration_only.or(self.emit_declaration_only);
    }
}

fn replace_path_option(
    compiler_options: &serde_json::Map<String, serde_json::Value>,
    name: &str,
    config_dir: &Path,
    substitution_base: &Path,
    option: &mut Option<PathBuf>,
) -> Option<()> {
    if let Some(value) = compiler_options.get(name) {
        let value = value.as_str()?;
        if value.is_empty() {
            return None;
        }
        let value = value.replace("${configDir}", &substitution_base.to_string_lossy());
        let path = Path::new(&value);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            config_dir.join(path)
        };
        *option = Some(normalize_path(&path)?);
    }
    Some(())
}

fn replace_bool_option(
    compiler_options: &serde_json::Map<String, serde_json::Value>,
    name: &str,
    option: &mut Option<bool>,
) -> Option<()> {
    if let Some(value) = compiler_options.get(name) {
        *option = Some(value.as_bool()?);
    }
    Some(())
}

/// Find the configs in the `extends` chain of `config_path` that exist but fail
/// to parse. `value` is the already parsed content of `config_path`.
///
/// Returns `(path, parser message)` pairs in walk order. A target that does not
/// resolve to a file is skipped, because this walk reports syntax only. The
/// walk follows string and array `extends` with the same lookup that
/// [`TsconfigOutputMap`] uses, and stops at [`MAX_EXTENDS_DEPTH`].
pub(super) fn malformed_extends_parents(
    config_path: &Path,
    value: &serde_json::Value,
) -> Vec<(PathBuf, String)> {
    let mut malformed = Vec::new();
    let mut visited = FxHashSet::default();
    visited.insert(config_path.to_path_buf());
    let mut frontier = extends_parents(config_path, value)
        .into_iter()
        .map(|parent| (parent, 1))
        .collect::<Vec<_>>();
    while let Some((path, depth)) = frontier.pop() {
        if depth > MAX_EXTENDS_DEPTH || !visited.insert(path.clone()) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        match crate::jsonc::parse_to_value::<serde_json::Value>(
            content.trim_start_matches('\u{FEFF}'),
        ) {
            Ok(parent_value) => frontier.extend(
                extends_parents(&path, &parent_value)
                    .into_iter()
                    .map(|parent| (parent, depth + 1)),
            ),
            Err(error) => malformed.push((path, error.to_string())),
        }
    }
    malformed
}

/// The tsconfig files that `config_path` pulls in, transitively, through
/// `extends` (one string or an array) and `references[].path`.
///
/// The result excludes `config_path` and is in walk order. A target that does
/// not resolve to a file, or a file that does not parse, ends the walk on that
/// branch. The walk stops after 16 levels, the same limit as the `extends`
/// walk of [`TsconfigOutputMap`].
#[must_use]
pub fn tsconfig_linked_files(config_path: &Path) -> Vec<PathBuf> {
    let mut visited = FxHashSet::default();
    visited.insert(config_path.to_path_buf());
    let mut linked = Vec::new();
    let mut frontier = vec![(config_path.to_path_buf(), 0)];
    while let Some((path, depth)) = frontier.pop() {
        if depth >= MAX_EXTENDS_DEPTH {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = crate::jsonc::parse_to_value::<serde_json::Value>(
            content.trim_start_matches('\u{FEFF}'),
        ) else {
            continue;
        };
        let targets = extends_parents(&path, &value)
            .into_iter()
            .chain(reference_targets(&path, &value));
        for target in targets {
            if visited.insert(target.clone()) {
                linked.push(target.clone());
                frontier.push((target, depth + 1));
            }
        }
    }
    linked
}

/// The config files that `references[].path` names. A reference path is a
/// config file or a directory that holds a `tsconfig.json`.
fn reference_targets(config_path: &Path, value: &serde_json::Value) -> Vec<PathBuf> {
    let (Some(config_dir), Some(references)) = (
        config_path.parent(),
        value
            .get("references")
            .and_then(serde_json::Value::as_array),
    ) else {
        return Vec::new();
    };
    references
        .iter()
        .filter_map(|reference| reference.get("path")?.as_str())
        .filter_map(|target| {
            let target = normalize_path(&config_dir.join(target))?;
            if target.is_file() {
                return Some(target);
            }
            let directory_config = target.join("tsconfig.json");
            directory_config.is_file().then_some(directory_config)
        })
        .collect()
}

/// The resolved parent configs of one tsconfig: `extends` is one string or,
/// since TypeScript 5.0, an array of strings.
fn extends_parents(config_path: &Path, value: &serde_json::Value) -> Vec<PathBuf> {
    let targets: Vec<&str> = match value.get("extends") {
        Some(serde_json::Value::String(target)) => vec![target.as_str()],
        Some(serde_json::Value::Array(items)) => {
            items.iter().filter_map(serde_json::Value::as_str).collect()
        }
        _ => Vec::new(),
    };
    targets
        .into_iter()
        .filter_map(|target| resolve_extends_path(config_path, target))
        .collect()
}

fn resolve_extends_path(config_path: &Path, extends: &str) -> Option<PathBuf> {
    let path = Path::new(extends);
    let config_dir = config_path.parent()?;
    let explicitly_local =
        path.is_absolute() || extends.starts_with("./") || extends.starts_with("../");
    if !explicitly_local {
        return resolve_package_extends(config_dir, extends);
    }
    let path = normalize_path(&config_dir.join(path))?;
    if path.is_file() {
        return Some(path);
    }
    let directory_config = path.join("tsconfig.json");
    if directory_config.is_file() {
        return Some(directory_config);
    }
    let file_name = path.file_name()?.to_string_lossy();
    if !file_name.ends_with(".json") && !file_name.ends_with(".jsonc") {
        let json = append_path_suffix(&path, ".json")?;
        if json.is_file() {
            return Some(json);
        }
        let jsonc = append_path_suffix(&path, ".jsonc")?;
        if jsonc.is_file() {
            return Some(jsonc);
        }
    }
    None
}

fn resolve_package_extends(config_dir: &Path, extends: &str) -> Option<PathBuf> {
    let (package_name, subpath) = package_extends_parts(extends)?;
    let mut directory = config_dir.to_path_buf();

    for _ in 0..MAX_EXTENDS_DEPTH {
        let package_root = directory.join("node_modules").join(&package_name);
        if package_root.is_dir() {
            if let Some(subpath) = subpath.as_deref() {
                return package_subpath_tsconfig(&package_root, subpath);
            }
            return package_root_tsconfig(&package_root);
        }
        if !directory.pop() {
            break;
        }
    }
    None
}

fn package_subpath_tsconfig(package_root: &Path, subpath: &str) -> Option<PathBuf> {
    let package_json_path = package_root.join("package.json");
    if package_json_path.is_file() {
        let package_json = std::fs::read_to_string(package_json_path).ok()?;
        let package_json: serde_json::Value = serde_json::from_str(&package_json).ok()?;
        if let Some(exports) = package_json.get("exports") {
            let serde_json::Value::Object(exports) = exports else {
                return None;
            };
            let target = exported_subpath_target(exports, &format!("./{subpath}"))?;
            return resolve_exported_config_file(package_root, &target);
        }
    }
    resolve_config_file(package_root, &format!("./{subpath}"))
}

/// Find the string target of an `exports` key: the exact key first, then the
/// `*` pattern with the longest prefix, as Node.js matches them.
fn exported_subpath_target(
    exports: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    if let Some(target) = exports.get(key) {
        return target.as_str().map(str::to_string);
    }
    let (_, target, matched) = exports
        .iter()
        .filter_map(|(pattern, target)| {
            let (prefix, suffix) = pattern.split_once('*')?;
            if suffix.contains('*') || key.len() < prefix.len() + suffix.len() {
                return None;
            }
            let matched = key.strip_prefix(prefix)?.strip_suffix(suffix)?;
            Some((prefix.len(), target, matched))
        })
        .max_by_key(|(prefix_len, _, _)| *prefix_len)?;
    Some(target.as_str()?.replace('*', matched))
}

fn package_extends_parts(extends: &str) -> Option<(PathBuf, Option<String>)> {
    let parts = extends.split('/').collect::<Vec<_>>();
    let package_parts = if parts.first()?.starts_with('@') {
        2
    } else {
        1
    };
    if parts.len() < package_parts || parts.iter().any(|part| part.is_empty() || *part == "..") {
        return None;
    }
    let package = parts[..package_parts].join("/");
    let subpath = (parts.len() > package_parts).then(|| parts[package_parts..].join("/"));
    Some((PathBuf::from(package), subpath))
}

fn package_root_tsconfig(package_root: &Path) -> Option<PathBuf> {
    let package_json_path = package_root.join("package.json");
    if package_json_path.is_file() {
        let package_json = std::fs::read_to_string(package_json_path).ok()?;
        let package_json: serde_json::Value = serde_json::from_str(&package_json).ok()?;
        if let Some(exports) = package_json.get("exports") {
            let target = match exports {
                serde_json::Value::String(target) => Some(target.as_str()),
                serde_json::Value::Object(map) => map.get(".").and_then(serde_json::Value::as_str),
                _ => None,
            }?;
            return resolve_exported_config_file(package_root, target);
        }
        if let Some(tsconfig) = package_json.get("tsconfig") {
            return resolve_package_tsconfig_field(package_root, tsconfig.as_str()?);
        }
    }
    let conventional = package_root.join("tsconfig.json");
    conventional.is_file().then_some(conventional)
}

fn resolve_package_tsconfig_field(package_root: &Path, target: &str) -> Option<PathBuf> {
    if target.starts_with("./") {
        return resolve_config_file(package_root, target);
    }
    if target.is_empty()
        || target.starts_with('/')
        || target.contains('*')
        || Path::new(target)
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    resolve_config_file(package_root, &format!("./{target}"))
}

fn resolve_exported_config_file(root: &Path, target: &str) -> Option<PathBuf> {
    let path = resolve_config_target(root, target)?;
    (path.extension()? == "json" && path.is_file()).then_some(path)
}

fn resolve_config_file(root: &Path, target: &str) -> Option<PathBuf> {
    let path = resolve_config_target(root, target)?;
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
        && path.is_file()
    {
        return Some(path);
    }
    let candidate = append_path_suffix(&path, ".json")?;
    candidate.is_file().then_some(candidate)
}

fn resolve_config_target(root: &Path, target: &str) -> Option<PathBuf> {
    if !target.starts_with("./") || target.contains('*') {
        return None;
    }
    let relative = Path::new(target);
    relative.file_name()?;
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    normalize_path(&root.join(relative))
}

fn source_suffix_for_output(
    entry: &Path,
    out_dir: &Path,
) -> Option<(PathBuf, &'static [&'static str])> {
    let relative = entry.strip_prefix(out_dir).ok()?;
    let file_name = relative.file_name()?.to_str()?;
    output_source_stem(relative, file_name)
}

fn output_source_stem(path: &Path, file_name: &str) -> Option<(PathBuf, &'static [&'static str])> {
    if let Some(suffix) = DECLARATION_OUTPUT_SUFFIXES
        .iter()
        .find(|suffix| file_name.ends_with(**suffix))
    {
        let stem = file_name.strip_suffix(suffix)?;
        let extensions = match *suffix {
            ".d.ts" => &["ts", "tsx", "js", "jsx"][..],
            ".d.mts" => &["mts", "mjs"][..],
            ".d.cts" => &["cts", "cjs"][..],
            _ => return None,
        };
        return Some((path.parent()?.join(stem), extensions));
    }

    let extension = path.extension()?.to_str()?;
    let stem = file_name.strip_suffix(&format!(".{extension}"))?;
    let extensions = match extension {
        "js" => &["ts", "tsx", "js", "jsx"][..],
        "jsx" => &["tsx", "jsx"][..],
        "mjs" => &["mts", "mjs"][..],
        "cjs" => &["cts", "cjs"][..],
        _ => return None,
    };
    Some((path.parent()?.join(stem), extensions))
}

fn append_extension(stem: &Path, extension: &str) -> Option<PathBuf> {
    let file_name = stem.file_name()?.to_str()?;
    Some(stem.parent()?.join(format!("{file_name}.{extension}")))
}

fn append_path_suffix(path: &Path, suffix: &str) -> Option<PathBuf> {
    let file_name = path.file_name()?.to_str()?;
    Some(path.parent()?.join(format!("{file_name}{suffix}")))
}

fn normalize_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    Some(normalized)
}

#[cfg(test)]
#[path = "tsconfig_tests.rs"]
mod tests;
