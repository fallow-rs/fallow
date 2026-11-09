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
        let entry_path = Path::new(entry);
        if entry_path.is_absolute()
            || entry_path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return TsconfigOutputResolution::Unconfigured;
        }
        let Some(entry_path) = normalize_path(&root.join(entry_path)) else {
            return TsconfigOutputResolution::Unconfigured;
        };
        // Candidates of a config with an explicit `rootDir` win over the
        // candidates of a config folder default, so an inferred `rootDir` can
        // not make an explicit mapping ambiguous.
        let mut candidates = FxHashSet::default();
        let mut inferred_candidates = FxHashSet::default();
        let mut configured = false;

        for options in &self.configs {
            let Some(file_name) = entry_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let is_declaration = DECLARATION_OUTPUT_SUFFIXES
                .iter()
                .any(|suffix| file_name.ends_with(suffix));
            // With `emitDeclarationOnly`, another tool (swc, Babel, a bundler)
            // can emit the JavaScript into `outDir` with the same layout. A
            // source file with the same stem then maps, but the config does not
            // claim the output, so the callers keep their other fallbacks.
            let claims_output = is_declaration || !options.emit_declaration_only.unwrap_or(false);
            let output_dir = if is_declaration {
                options
                    .declaration_dir
                    .as_deref()
                    .or(options.out_dir.as_deref())
            } else {
                options.out_dir.as_deref()
            };
            let Some(output_dir) = output_dir else {
                continue;
            };
            let Some((source_stem, family_extensions)) =
                source_suffix_for_output(&entry_path, output_dir)
            else {
                continue;
            };
            let (root_dir, target) = match (options.root_dir.as_deref(), root_dir_default) {
                (Some(root_dir), _) => {
                    configured |= claims_output;
                    (root_dir, &mut candidates)
                }
                (None, RootDirDefault::ConfigDir) => match options.config_dir.as_deref() {
                    Some(config_dir) => (config_dir, &mut inferred_candidates),
                    None => continue,
                },
                (None, RootDirDefault::Skip) => continue,
            };
            if options.no_emit.unwrap_or(false) {
                continue;
            }
            for extension in family_extensions {
                if !source_extensions.contains(extension) {
                    continue;
                }
                let Some(candidate) = append_extension(&source_stem, extension) else {
                    continue;
                };
                let candidate = root_dir.join(candidate);
                let Ok(candidate) = dunce::canonicalize(candidate) else {
                    continue;
                };
                if candidate.starts_with(root) && candidate.is_file() {
                    target.insert(candidate);
                }
            }
        }
        if candidates.is_empty() {
            candidates = inferred_candidates;
        }

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
        let Some(source) = candidates.into_iter().next() else {
            return TsconfigOutputResolution::ConfiguredButUnresolved;
        };
        let Some(relative) = source.strip_prefix(root).ok() else {
            return TsconfigOutputResolution::ConfiguredButUnresolved;
        };
        let Some(display_root) = self.display_root.as_deref() else {
            return TsconfigOutputResolution::ConfiguredButUnresolved;
        };
        TsconfigOutputResolution::Resolved(display_root.join(relative))
    }
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
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().expect("parent directory")).expect("create parent");
        std::fs::write(path, content).expect("write file");
    }

    fn map(root: &Path, entry: &str, source_extensions: &[&str]) -> TsconfigOutputResolution {
        TsconfigOutputMap::from_project(root).resolve_source_for_entry(entry, source_extensions)
    }

    #[test]
    fn maps_generated_entry_with_inherited_paths_relative_to_their_config_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("configs/tsconfig.base.json"),
            r#"{
                "compilerOptions": {
                    "rootDir": "../source",
                    "outDir": "../distribution"
                }
            }"#,
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{"extends":"./configs/tsconfig.base.json"}"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts", "tsx", "js"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );
    }

    #[test]
    fn linked_files_follow_extends_and_references_transitively() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path();
        write(
            &root.join("tsconfig.json"),
            r#"{
              // comments are allowed
              "extends": ["./configs/base.json"],
              "references": [{ "path": "./packages/app" }, { "path": "./missing" }]
            }"#,
        );
        write(
            &root.join("configs/base.json"),
            r#"{"extends":"./shared.json"}"#,
        );
        write(&root.join("configs/shared.json"), "{}");
        write(
            &root.join("packages/app/tsconfig.json"),
            r#"{"extends":"../../tsconfig.json"}"#,
        );

        let mut linked = tsconfig_linked_files(&root.join("tsconfig.json"));
        linked.sort();
        let mut expected = vec![
            root.join("configs/base.json"),
            root.join("configs/shared.json"),
            root.join("packages/app/tsconfig.json"),
        ];
        expected.sort();
        assert_eq!(linked, expected);
    }

    #[test]
    fn no_emit_configs_do_not_map_public_outputs() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("configs/tsconfig.base.json"),
            r#"{
                "compilerOptions": {
                    "rootDir": "../source",
                    "outDir": "../distribution"
                }
            }"#,
        );
        write(
            &root.join("tsconfig.test.json"),
            r#"{
                "extends": "./configs/tsconfig.base.json",
                "compilerOptions": {"noEmit": true}
            }"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::ConfiguredWithoutSource,
            "a noEmit config maps no output to a source file"
        );
        write(
            &root.join("tsconfig.test.json"),
            r#"{"extends":"./configs/tsconfig.base.json","compilerOptions":{"noEmit":false}}"#,
        );
        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );
    }

    /// A Nest build can declare `rootDir: "."` while the scripts run
    /// `dist/<path>.js` from `src/<path>.ts`. The config claims the output but
    /// names no file, which differs from two candidate files.
    #[test]
    fn emitting_config_without_a_source_file_is_reported_apart_from_ambiguity() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("tsconfig.build.json"),
            r#"{"compilerOptions":{"rootDir":".","outDir":"./dist"}}"#,
        );
        write(
            &root.join("src/database/setup-db.ts"),
            "export const value = 1;\n",
        );

        assert_eq!(
            map(root, "./dist/database/setup-db.js", &["ts"]),
            TsconfigOutputResolution::ConfiguredWithoutSource
        );
    }

    #[test]
    fn ambiguous_source_candidates_fail_closed() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("tsconfig.build.json"),
            r#"{
                "compilerOptions": {
                    "rootDir": "./source",
                    "outDir": "./distribution"
                }
            }"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");
        write(&root.join("source/index.js"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts", "js"]),
            TsconfigOutputResolution::ConfiguredButUnresolved
        );
        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );
    }

    #[test]
    fn maps_dotted_basenames_and_keeps_module_output_families_separate() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("tsconfig.build.json"),
            r#"{
                "compilerOptions": {
                    "rootDir": "./source",
                    "outDir": "./distribution"
                }
            }"#,
        );
        write(
            &root.join("source/feature.test.ts"),
            "export const value = 1;\n",
        );
        write(
            &root.join("source/feature.native.mts"),
            "export const native = 1;\n",
        );

        assert_eq!(
            map(root, "./distribution/feature.test.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/feature.test.ts"))
        );
        assert_eq!(
            map(root, "./distribution/feature.test.mjs", &["ts"]),
            TsconfigOutputResolution::ConfiguredWithoutSource,
            "an MJS output must not resolve to an unrelated TS source with the same basename"
        );
        assert_eq!(
            map(root, "./distribution/feature.native.mjs", &["mts"]),
            TsconfigOutputResolution::Resolved(root.join("source/feature.native.mts"))
        );
    }

    #[test]
    fn resolves_installed_package_extends_and_config_dir_options_statically() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("node_modules/@scope/config/package.json"),
            r#"{"exports":"./shared.json"}"#,
        );
        write(
            &root.join("node_modules/@scope/config/shared.json"),
            r#"{
                "compilerOptions": {
                    "outDir": "${configDir}/distribution"
                }
            }"#,
        );
        write(
            &root.join("tsconfig.json"),
            r#"{"extends":"@scope/config"}"#,
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{
                "extends":"./tsconfig.json",
                "compilerOptions":{"rootDir":"./source"}
            }"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );
    }

    #[test]
    fn bare_extends_uses_package_lookup_and_dotted_local_stems_append_json() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("configs/tsconfig.base.json"),
            r#"{
                "compilerOptions": {
                    "rootDir": "../source",
                    "outDir": "../local-output"
                }
            }"#,
        );
        write(
            &root.join("node_modules/shared-config/package.json"),
            r#"{"exports":"./shared.json"}"#,
        );
        write(
            &root.join("node_modules/shared-config/shared.json"),
            r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
        );
        write(
            &root.join("shared-config"),
            r#"{"compilerOptions":{"outDir":"./wrong-output"}}"#,
        );
        write(
            &root.join("tsconfig.local.json"),
            r#"{"extends":"./configs/tsconfig.base"}"#,
        );
        write(
            &root.join("tsconfig.package.json"),
            r#"{
                "extends":"shared-config",
                "compilerOptions":{"rootDir":"./source"}
            }"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        let map = TsconfigOutputMap::from_project(root);
        assert_eq!(
            map.resolve_source_for_entry("./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
            "bare package names must resolve through node_modules even with a same-named local file"
        );
        assert_eq!(
            map.resolve_source_for_entry("./local-output/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
            "dotted local config stems should resolve by appending .json"
        );
    }

    #[test]
    fn package_config_fields_and_unexported_subpaths_resolve_json_files() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        let package_root = root.join("node_modules/shared-config");
        for file in [
            "valid.json",
            "dotted.base.json",
            "double.json.json",
            "extensionless",
            "invalid.jsonc",
            "invalid.ts",
        ] {
            write(
                &package_root.join(file),
                r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
            );
        }
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        for use_field in [true, false] {
            for (target, resolves) in [
                ("valid.json", true),
                ("valid", true),
                ("dotted.base", true),
                ("double.json", true),
                ("extensionless", false),
                ("invalid.jsonc", false),
                ("invalid.ts", false),
            ] {
                let (manifest, extends) = if use_field {
                    (
                        serde_json::json!({"tsconfig": target}),
                        "shared-config".to_string(),
                    )
                } else {
                    (serde_json::json!({}), format!("shared-config/{target}"))
                };
                write(&package_root.join("package.json"), &manifest.to_string());
                write(
                    &root.join("tsconfig.json"),
                    &serde_json::json!({
                        "extends": extends,
                        "compilerOptions": {"rootDir": "./source"}
                    })
                    .to_string(),
                );
                let expected = if resolves {
                    TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
                } else {
                    TsconfigOutputResolution::Unconfigured
                };
                assert_eq!(
                    map(root, "./distribution/index.js", &["ts"]),
                    expected,
                    "package config target {target}, tsconfig field present={use_field}"
                );
            }
        }
    }

    #[test]
    fn package_exports_precede_conventional_tsconfig_and_conditions_fail_closed() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("node_modules/shared-config/package.json"),
            r#"{"exports":{".":"./build.json"}}"#,
        );
        write(
            &root.join("node_modules/shared-config/build.json"),
            r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
        );
        write(
            &root.join("node_modules/shared-config/tsconfig.json"),
            r#"{"compilerOptions":{"outDir":"../../other"}}"#,
        );
        write(
            &root.join("node_modules/conditional-config/package.json"),
            r#"{"exports":{".":{"types":"./types.json"}},"tsconfig":"./fallback.json"}"#,
        );
        write(
            &root.join("node_modules/conditional-config/types.json"),
            r#"{"compilerOptions":{"outDir":"../../conditional-output"}}"#,
        );
        write(
            &root.join("node_modules/conditional-config/fallback.json"),
            r#"{"compilerOptions":{"outDir":"../../conditional-output"}}"#,
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{
                "extends":"shared-config",
                "compilerOptions":{"rootDir":"./source"}
            }"#,
        );
        write(
            &root.join("tsconfig.conditional.json"),
            r#"{
                "extends":"conditional-config",
                "compilerOptions":{"rootDir":"./source"}
            }"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        let map = TsconfigOutputMap::from_project(root);
        assert_eq!(
            map.resolve_source_for_entry("./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
            "the simple root export should be selected before conventional tsconfig.json"
        );
        assert_eq!(
            map.resolve_source_for_entry("./other/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured,
            "a simple root export must take precedence over the conventional config"
        );
        assert_eq!(
            map.resolve_source_for_entry("./conditional-output/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured,
            "unsupported conditional package exports must not fall through to package tsconfig"
        );
    }

    #[test]
    fn package_export_targets_require_explicit_config_extensions() {
        for (extends, export_key) in [
            ("shared-config", None),
            ("shared-config", Some(".")),
            ("shared-config/build", Some("./build")),
        ] {
            let directory = tempfile::tempdir().expect("temporary directory");
            let root = directory.path();
            let package_root = root.join("node_modules/shared-config");
            for file in [
                "build.json",
                "build",
                "build.jsonc",
                "build.ts",
                "inferred.json",
                "double.json.json",
            ] {
                write(
                    &package_root.join(file),
                    r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
                );
            }
            write(
                &root.join("tsconfig.json"),
                &format!(r#"{{"extends":"{extends}","compilerOptions":{{"rootDir":"./source"}}}}"#),
            );
            write(&root.join("source/index.ts"), "export const value = 1;\n");

            for (target, expected) in [
                ("./build", TsconfigOutputResolution::Unconfigured),
                ("./build.jsonc", TsconfigOutputResolution::Unconfigured),
                ("./build.ts", TsconfigOutputResolution::Unconfigured),
                ("./inferred", TsconfigOutputResolution::Unconfigured),
                ("./double.json", TsconfigOutputResolution::Unconfigured),
                (
                    "./build.json",
                    TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
                ),
            ] {
                let exports = export_key.map_or_else(
                    || serde_json::json!(target),
                    |key| serde_json::json!({key: target}),
                );
                write(
                    &package_root.join("package.json"),
                    &serde_json::json!({"exports": exports}).to_string(),
                );
                assert_eq!(
                    map(root, "./distribution/index.js", &["ts"]),
                    expected,
                    "package exports must resolve exactly: {extends} -> {target}"
                );
            }
        }
    }

    #[test]
    fn package_subpaths_resolve_through_simple_exports_without_bypassing_restrictions() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("node_modules/shared-config/package.json"),
            r#"{"exports":{".":"./root.json","./build":"./build.json"}}"#,
        );
        write(
            &root.join("node_modules/shared-config/root.json"),
            r#"{"compilerOptions":{"outDir":"../../wrong-output"}}"#,
        );
        write(
            &root.join("node_modules/shared-config/build.json"),
            r#"{"compilerOptions":{"outDir":"../../distribution"}}"#,
        );
        write(
            &root.join("node_modules/shared-config/private.json"),
            r#"{"compilerOptions":{"outDir":"../../private-output"}}"#,
        );
        write(
            &root.join("node_modules/restricted-config/package.json"),
            r#"{"exports":"./root.json"}"#,
        );
        write(
            &root.join("node_modules/restricted-config/root.json"),
            r#"{"compilerOptions":{"outDir":"../../wrong-output"}}"#,
        );
        write(
            &root.join("node_modules/restricted-config/private.json"),
            r#"{"compilerOptions":{"outDir":"../../private-output"}}"#,
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{
                "extends":"shared-config/build",
                "compilerOptions":{"rootDir":"./source"}
            }"#,
        );
        write(
            &root.join("tsconfig.restricted.json"),
            r#"{
                "extends":"restricted-config/private",
                "compilerOptions":{"rootDir":"./source"}
            }"#,
        );
        write(
            &root.join("tsconfig.private.json"),
            r#"{"extends":"shared-config/private","compilerOptions":{"rootDir":"./source"}}"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        let map = TsconfigOutputMap::from_project(root);
        assert_eq!(
            map.resolve_source_for_entry("./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
            "the exact exported subpath should map through the configured build output"
        );
        assert_eq!(
            map.resolve_source_for_entry("./wrong-output/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured,
            "a subpath must not use the package root config"
        );
        assert_eq!(
            map.resolve_source_for_entry("./private-output/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured,
            "an unexported package subpath must not be loaded from disk"
        );
    }

    #[test]
    fn package_subpaths_resolve_through_exports_patterns() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("node_modules/@scope/config/package.json"),
            r#"{"exports":{"./base.json":"./base.json","./*":"./*","./presets/*.json":"./generated/*.json"}}"#,
        );
        write(
            &root.join("node_modules/@scope/config/modern/build.json"),
            r#"{"compilerOptions":{"outDir":"../../../../distribution"}}"#,
        );
        write(
            &root.join("node_modules/@scope/config/generated/strict.json"),
            r#"{"compilerOptions":{"outDir":"../../../../preset-output"}}"#,
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{"extends":"@scope/config/modern/build.json","compilerOptions":{"rootDir":"./source"}}"#,
        );
        write(
            &root.join("tsconfig.preset.json"),
            r#"{"extends":"@scope/config/presets/strict.json","compilerOptions":{"rootDir":"./source"}}"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        let map = TsconfigOutputMap::from_project(root);
        assert_eq!(
            map.resolve_source_for_entry("./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
            "a `./*` pattern exports the subpath"
        );
        assert_eq!(
            map.resolve_source_for_entry("./preset-output/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
            "the pattern with the longest prefix wins"
        );
    }

    #[test]
    fn missing_local_extends_does_not_use_child_options() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("tsconfig.build.json"),
            r#"{
                "extends":"./missing-config.json",
                "compilerOptions": {
                    "rootDir":"./source",
                    "outDir":"./distribution"
                }
            }"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured
        );
        write(&root.join("missing-config.json"), "{}");
        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );
    }

    #[test]
    fn emit_declaration_only_maps_javascript_output_to_source() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("tsconfig.json"),
            r#"{"compilerOptions":{"rootDir":"./src","outDir":"./dist","emitDeclarationOnly":true,"declaration":true}}"#,
        );
        write(&root.join("src/cli.ts"), "export const main = () => 1;\n");

        for output in ["./dist/cli.js", "./dist/cli.d.ts"] {
            assert_eq!(
                map(root, output, &["ts"]),
                TsconfigOutputResolution::Resolved(root.join("src/cli.ts")),
                "{output} maps to source"
            );
        }
        assert_eq!(
            map(root, "./dist/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured,
            "JavaScript output without a source file stays open for other fallbacks"
        );
        assert_eq!(
            map(root, "./dist/index.d.ts", &["ts"]),
            TsconfigOutputResolution::ConfiguredWithoutSource
        );
    }

    #[test]
    fn array_extends_applies_parents_in_order_before_own_options() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("configs/first.json"),
            r#"{"compilerOptions":{"rootDir":"../wrong","outDir":"../distribution"}}"#,
        );
        write(
            &root.join("configs/second.json"),
            r#"{"compilerOptions":{"rootDir":"../source"}}"#,
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{"extends":["./configs/first.json","./configs/second.json"]}"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );
    }

    #[test]
    fn output_path_without_root_dir_maps_from_the_config_folder() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(&root.join("tsconfig.json"), r#"{"compilerOptions":{}}"#);
        write(
            &root.join("tsconfig.build.json"),
            r#"{"extends":["./tsconfig.json"],"compilerOptions":{"outDir":"dist"}}"#,
        );
        write(
            &root.join("nodes/Http/transport.ts"),
            "export const t = 1;\n",
        );

        let output_map = TsconfigOutputMap::from_project(root);
        assert_eq!(
            output_map.resolve_source_for_output_path(
                &root.join("dist/nodes/Http/transport.js"),
                &["ts"]
            ),
            TsconfigOutputResolution::Resolved(root.join("nodes/Http/transport.ts"))
        );
        assert_eq!(
            output_map.resolve_source_for_output_path(&root.join("dist/missing.js"), &["ts"]),
            TsconfigOutputResolution::Unconfigured,
            "an inferred rootDir does not claim the output"
        );
        assert_eq!(
            map(root, "./dist/nodes/Http/transport.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured,
            "package entries keep the explicit rootDir rule"
        );
    }

    #[test]
    fn explicit_root_dir_wins_over_the_config_folder_default() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("tsconfig.json"),
            r#"{"compilerOptions":{"rootDir":"src","outDir":"dist"}}"#,
        );
        write(
            &root.join("tsconfig.other.json"),
            r#"{"compilerOptions":{"outDir":"dist"}}"#,
        );
        write(&root.join("src/util.ts"), "export const a = 1;\n");
        write(&root.join("util.ts"), "export const b = 1;\n");
        write(&root.join("only-root.ts"), "export const c = 1;\n");

        let output_map = TsconfigOutputMap::from_project(root);
        assert_eq!(
            output_map.resolve_source_for_output_path(&root.join("dist/util.js"), &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("src/util.ts"))
        );
        assert_eq!(
            output_map.resolve_source_for_output_path(&root.join("dist/only-root.js"), &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("only-root.ts")),
            "the config folder default still applies when the explicit rootDir has no source"
        );
    }

    #[test]
    fn invalid_configs_fail_closed() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(&root.join("source/index.ts"), "export const value = 1;\n");
        write(
            &root.join("tsconfig.build.json"),
            "{ this is not valid JSONC",
        );

        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{"compilerOptions":{"rootDir":"./source","outDir":"./distribution"}}"#,
        );
        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );

        let missing = tempfile::tempdir().expect("empty temporary project");
        assert_eq!(
            map(missing.path(), "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured
        );
    }

    /// Run `map` on a separate thread and fail when it does not finish in time.
    fn map_within(
        root: &Path,
        entry: &'static str,
        limit: std::time::Duration,
    ) -> TsconfigOutputResolution {
        let root = root.to_path_buf();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(map(&root, entry, &["ts"]));
        });
        receiver
            .recv_timeout(limit)
            .expect("the extends walk must finish within the time limit")
    }

    #[test]
    fn wide_array_extends_diamond_reads_each_config_once() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write_extends_diamond(root, 8, 13, 1, 0);

        assert_eq!(
            map_within(
                root,
                "./distribution/index.js",
                std::time::Duration::from_secs(10)
            ),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
            "the last extends entry of each level must win"
        );
    }

    #[test]
    fn repeated_array_extends_entries_are_followed_once() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write_extends_diamond(root, 8, 12, 400, 0);

        assert_eq!(
            map_within(
                root,
                "./distribution/index.js",
                std::time::Duration::from_secs(5)
            ),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
        );
    }

    #[test]
    fn extends_walk_over_the_entry_limit_fails_closed() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("configs/base.json"),
            r#"{"compilerOptions":{"rootDir":"../source","outDir":"../distribution"}}"#,
        );
        // Each specifier is a different string for the same config.
        let parents = (0..=MAX_EXTENDS_ENTRIES)
            .map(|index| format!(r#""./configs/folder-{index}/../base.json""#))
            .collect::<Vec<_>>()
            .join(",");
        write(
            &root.join("tsconfig.build.json"),
            &format!(r#"{{"extends":[{parents}]}}"#),
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured
        );
    }

    #[test]
    fn root_config_over_a_limit_does_not_disable_other_root_configs() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("configs/base.json"),
            r#"{"compilerOptions":{"rootDir":"../code","outDir":"../output"}}"#,
        );
        write(
            &root.join("configs/shared.json"),
            r#"{"extends":"./base.json"}"#,
        );
        write(&root.join("configs/leaf.json"), "{}");
        // `tsconfig.a.json` sorts before `tsconfig.json`. Its own entries use
        // the whole entry budget, so the walk goes over the limit inside the
        // shared config.
        let parents = (0..MAX_EXTENDS_ENTRIES - 1)
            .map(|index| format!(r#""./configs/folder-{index}/../leaf.json""#))
            .chain(std::iter::once(r#""./configs/shared.json""#.to_owned()))
            .collect::<Vec<_>>()
            .join(",");
        write(
            &root.join("tsconfig.a.json"),
            &format!(r#"{{"extends":[{parents}]}}"#),
        );
        write(
            &root.join("tsconfig.json"),
            r#"{"extends":"./configs/shared.json"}"#,
        );
        write(&root.join("code/index.ts"), "export const value = 1;\n");

        let output_map = TsconfigOutputMap::from_project(root);
        assert_eq!(
            output_map.configs.len(),
            1,
            "only the root config over the limit fails closed"
        );
        assert_eq!(
            output_map.resolve_source_for_entry("./output/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("code/index.ts"))
        );
    }

    #[test]
    fn repeated_extends_entry_keeps_its_last_place() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("configs/first.json"),
            r#"{"compilerOptions":{"rootDir":"../source","outDir":"../distribution"}}"#,
        );
        write(
            &root.join("configs/second.json"),
            r#"{"compilerOptions":{"rootDir":"../wrong"}}"#,
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{"extends":["./configs/first.json","./configs/second.json","./configs/first.json"]}"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );
    }

    #[test]
    fn many_root_configs_over_one_extends_graph_share_the_walk() {
        const EXTRA_ROOTS: usize = 40;
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path().to_path_buf();
        write_extends_diamond(&root, 8, 12, 10, EXTRA_ROOTS);

        let (sender, receiver) = std::sync::mpsc::channel();
        let walk_root = root.clone();
        std::thread::spawn(move || {
            let _ = sender.send(TsconfigOutputMap::from_project(&walk_root));
        });
        let output_map = receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the extends walk must finish within the time limit");
        assert_eq!(
            output_map.configs.len(),
            EXTRA_ROOTS + 1,
            "every root config must stay within the shared walk limits"
        );
        assert_eq!(
            output_map.resolve_source_for_entry("./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts"))
        );
    }

    /// Write a project where `tsconfig.build.json` and `extra_roots` other
    /// root configs extend a diamond of `levels` levels of `width` configs.
    /// Every config of a level extends every config of the next level, each
    /// entry `duplicates` times. Only the last config of the last level sets
    /// the correct `rootDir`.
    fn write_extends_diamond(
        root: &Path,
        width: usize,
        levels: usize,
        duplicates: usize,
        extra_roots: usize,
    ) {
        for level in 0..levels {
            for index in 0..width {
                let content = if level + 1 == levels {
                    let root_dir = if index + 1 == width {
                        "../source"
                    } else {
                        "../wrong"
                    };
                    format!(
                        r#"{{"compilerOptions":{{"rootDir":"{root_dir}","outDir":"../distribution"}}}}"#
                    )
                } else {
                    let parents = (0..duplicates)
                        .flat_map(|_| 0..width)
                        .map(|parent| format!(r#""./level-{}-{parent}.json""#, level + 1))
                        .collect::<Vec<_>>()
                        .join(",");
                    format!(r#"{{"extends":[{parents}]}}"#)
                };
                write(
                    &root.join(format!("configs/level-{level}-{index}.json")),
                    &content,
                );
            }
        }
        let top = (0..width)
            .map(|index| format!(r#""./configs/level-0-{index}.json""#))
            .collect::<Vec<_>>()
            .join(",");
        let top = format!(r#"{{"extends":[{top}]}}"#);
        write(&root.join("tsconfig.build.json"), &top);
        for extra in 0..extra_roots {
            write(&root.join(format!("tsconfig.extra-{extra}.json")), &top);
        }
        write(&root.join("source/index.ts"), "export const value = 1;\n");
    }

    #[test]
    fn extends_walk_over_the_read_limit_fails_closed() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        let parents = (0..=MAX_EXTENDS_READS)
            .map(|index| {
                write(
                    &root.join(format!("configs/base-{index}.json")),
                    r#"{"compilerOptions":{"rootDir":"../source","outDir":"../distribution"}}"#,
                );
                format!(r#""./configs/base-{index}.json""#)
            })
            .collect::<Vec<_>>()
            .join(",");
        write(
            &root.join("tsconfig.build.json"),
            &format!(r#"{{"extends":[{parents}]}}"#),
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured
        );
    }

    #[test]
    fn shared_parent_in_two_extends_branches_is_not_a_cycle() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        write(
            &root.join("configs/shared.json"),
            r#"{"compilerOptions":{"rootDir":"../source","outDir":"../distribution"}}"#,
        );
        write(
            &root.join("configs/first.json"),
            r#"{"extends":"./shared.json","compilerOptions":{"rootDir":"../wrong"}}"#,
        );
        write(
            &root.join("configs/second.json"),
            r#"{"extends":"./shared.json"}"#,
        );
        write(
            &root.join("tsconfig.build.json"),
            r#"{"extends":["./configs/first.json","./configs/second.json"]}"#,
        );
        write(&root.join("source/index.ts"), "export const value = 1;\n");

        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Resolved(root.join("source/index.ts")),
            "the second branch re-applies the shared parent over the first branch"
        );

        write(
            &root.join("configs/shared.json"),
            r#"{"extends":"../tsconfig.build.json"}"#,
        );
        assert_eq!(
            map(root, "./distribution/index.js", &["ts"]),
            TsconfigOutputResolution::Unconfigured,
            "a real cycle still fails closed"
        );
    }
}
