//! Report production files that import a file the production patterns
//! excluded (issue #4498).
//!
//! An import of an excluded file resolves to [`ResolveResult::ExternalFile`],
//! because discovery gave the file no [`FileId`]. This pass matches those
//! targets against [`PRODUCTION_EXCLUDE_PATTERNS`], so it reads no excluded
//! file and runs no second analysis.
//!
//! An import of a workspace package keeps no file path in its result. For an
//! import whose specifier names a workspace package, the pass reads the entry
//! fields of that package manifest (`main`, `module`, `source` and the
//! matching `exports` key) and matches the entry files that exist.

use std::path::{Path, PathBuf};

use fallow_config::{
    PackageJson, ResolvedConfig, WorkspaceDiagnostic, WorkspaceDiagnosticKind, WorkspaceInfo,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover::{FileId, PRODUCTION_EXCLUDE_PATTERNS, SOURCE_EXTENSIONS};
use crate::resolve::{ResolveResult, ResolvedModule};

/// The number of excluded files that one diagnostic names.
const SAMPLE_LIMIT: usize = 5;

/// One `production-excluded-file-imported` diagnostic for each production
/// pattern that excluded a file that an analyzed file imports.
///
/// `excluded` holds the analyzed files that take no part in a production run
/// (support-only entries). Their imports are not production imports.
pub(super) fn diagnostics(
    config: &ResolvedConfig,
    resolved_modules: &[ResolvedModule],
    workspaces: &[WorkspaceInfo],
    excluded: &FxHashSet<FileId>,
) -> Vec<WorkspaceDiagnostic> {
    if !config.production {
        return Vec::new();
    }
    let Some(matcher) = PatternMatcher::new() else {
        return Vec::new();
    };
    let mut collector = Collector {
        config,
        matcher: &matcher,
        canonical_root: dunce::canonicalize(&config.root).ok(),
        seen: FxHashSet::default(),
        by_pattern: FxHashMap::default(),
    };
    let mut manifests = WorkspaceManifests::new(workspaces);
    let mut searched: FxHashSet<&str> = FxHashSet::default();

    for module in resolved_modules
        .iter()
        .filter(|module| !excluded.contains(&module.file_id))
    {
        for (specifier, target) in import_edges(module) {
            if let ResolveResult::ExternalFile(path) = target {
                collector.consider(path);
            } else if searched.insert(specifier) {
                for entry in manifests.entry_files(specifier) {
                    if entry.is_file() {
                        collector.consider(&entry);
                    }
                }
            }
        }
    }
    collector.finish()
}

/// The dropped files found so far, grouped by the index of their pattern.
struct Collector<'a> {
    config: &'a ResolvedConfig,
    matcher: &'a PatternMatcher,
    canonical_root: Option<PathBuf>,
    /// Root-relative paths, so one file reached through two spellings of the
    /// root (a symlink such as `/tmp` on macOS) counts once.
    seen: FxHashSet<String>,
    by_pattern: FxHashMap<usize, Vec<String>>,
}

impl Collector<'_> {
    fn consider(&mut self, path: &Path) {
        if !has_source_extension(path) {
            return;
        }
        let Some(relative) =
            relative_to_root(path, &self.config.root, self.canonical_root.as_deref())
        else {
            return;
        };
        if self.config.ignore_patterns.is_match(relative) {
            return;
        }
        let Some(index) = self.matcher.first_match(relative) else {
            return;
        };
        let key = relative.to_string_lossy().replace('\\', "/");
        if self.seen.insert(key.clone()) {
            self.by_pattern.entry(index).or_default().push(key);
        }
    }

    fn finish(self) -> Vec<WorkspaceDiagnostic> {
        let mut entries: Vec<(usize, Vec<String>)> = self.by_pattern.into_iter().collect();
        entries.sort_unstable_by_key(|(index, _)| *index);
        entries
            .into_iter()
            .map(|(index, mut files)| {
                files.sort_unstable();
                diagnostic(self.config, self.matcher.patterns[index], files)
            })
            .collect()
    }
}

/// The specifier and the resolved target of each static import, dynamic
/// import and re-export of `module`.
fn import_edges(module: &ResolvedModule) -> impl Iterator<Item = (&str, &ResolveResult)> {
    module
        .resolved_imports
        .iter()
        .chain(&module.resolved_dynamic_imports)
        .map(|import| (import.info.source.as_str(), &import.target))
        .chain(
            module
                .re_exports
                .iter()
                .map(|re_export| (re_export.info.source.as_str(), &re_export.target)),
        )
}

/// The manifests of the workspace packages, read on first use.
struct WorkspaceManifests<'a> {
    workspaces: &'a [WorkspaceInfo],
    loaded: FxHashMap<usize, Option<PackageJson>>,
}

impl<'a> WorkspaceManifests<'a> {
    fn new(workspaces: &'a [WorkspaceInfo]) -> Self {
        Self {
            workspaces,
            loaded: FxHashMap::default(),
        }
    }

    /// The entry files that a specifier of a workspace package selects:
    /// `main`, `module`, `source` and `exports["."]` for the bare name, and
    /// `exports["./<subpath>"]` for a subpath. Empty for any other specifier.
    fn entry_files(&mut self, specifier: &str) -> Vec<PathBuf> {
        let Some((index, subpath)) = self.workspaces.iter().enumerate().find_map(|(index, ws)| {
            let rest = specifier.strip_prefix(ws.name.as_str())?;
            match rest.strip_prefix('/') {
                Some(subpath) => Some((index, Some(subpath))),
                None => rest.is_empty().then_some((index, None)),
            }
        }) else {
            return Vec::new();
        };
        let workspace = &self.workspaces[index];
        let Some(manifest) = self
            .loaded
            .entry(index)
            .or_insert_with(|| fallow_config::load_dir_package_json(&workspace.root))
            .as_ref()
        else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        match subpath {
            None => {
                entries.extend(
                    [&manifest.main, &manifest.module, &manifest.source]
                        .into_iter()
                        .flatten()
                        .cloned(),
                );
                if let Some(exports) = &manifest.exports {
                    collect_export_targets(export_entry(exports, "."), &mut entries);
                }
            }
            Some(subpath) => {
                if let Some(exports) = &manifest.exports {
                    let key = format!("./{subpath}");
                    collect_export_targets(export_entry(exports, &key), &mut entries);
                }
            }
        }
        entries
            .iter()
            .map(|entry| workspace.root.join(entry.trim_start_matches("./")))
            .collect()
    }
}

/// The `exports` value for `key`. An `exports` object without `.`-prefixed
/// keys is a condition map for the bare name.
fn export_entry<'v>(exports: &'v serde_json::Value, key: &str) -> Option<&'v serde_json::Value> {
    match exports {
        serde_json::Value::Object(map) if map.keys().any(|k| k.starts_with('.')) => map.get(key),
        _ => (key == ".").then_some(exports),
    }
}

/// Every string target in an `exports` value, through nested conditions.
fn collect_export_targets(value: Option<&serde_json::Value>, out: &mut Vec<String>) {
    match value {
        Some(serde_json::Value::String(target)) => out.push(target.clone()),
        Some(serde_json::Value::Object(conditions)) => {
            for condition in conditions.values() {
                collect_export_targets(Some(condition), out);
            }
        }
        Some(serde_json::Value::Array(alternatives)) => {
            for alternative in alternatives {
                collect_export_targets(Some(alternative), out);
            }
        }
        _ => {}
    }
}

fn diagnostic(config: &ResolvedConfig, pattern: &str, files: Vec<String>) -> WorkspaceDiagnostic {
    let anchor = config.root.join(&files[0]);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "file count is bounded by project size, well under u32::MAX"
    )]
    let file_count = files.len() as u32;
    let mut sample = files;
    sample.truncate(SAMPLE_LIMIT);
    WorkspaceDiagnostic::new(
        &config.root,
        anchor,
        WorkspaceDiagnosticKind::ProductionExcludedFileImported {
            pattern: pattern.to_owned(),
            file_count,
            files: sample,
        },
    )
    .into_root_relative(&config.root)
}

fn has_source_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext))
}

fn relative_to_root<'a>(
    path: &'a Path,
    root: &Path,
    canonical_root: Option<&Path>,
) -> Option<&'a Path> {
    path.strip_prefix(root)
        .ok()
        .or_else(|| canonical_root.and_then(|canonical| path.strip_prefix(canonical).ok()))
}

/// The production patterns, compiled with the options that discovery uses,
/// and the pattern text of each compiled glob in the same order.
struct PatternMatcher {
    set: globset::GlobSet,
    patterns: Vec<&'static str>,
}

impl PatternMatcher {
    fn new() -> Option<Self> {
        let mut builder = globset::GlobSetBuilder::new();
        let mut patterns = Vec::with_capacity(PRODUCTION_EXCLUDE_PATTERNS.len());
        for pattern in PRODUCTION_EXCLUDE_PATTERNS {
            if let Ok(glob) = globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
            {
                builder.add(glob);
                patterns.push(*pattern);
            }
        }
        let set = builder.build().ok()?;
        Some(Self { set, patterns })
    }

    /// The index of the first pattern in built-in order that matches.
    fn first_match(&self, relative: &Path) -> Option<usize> {
        self.set.matches(relative).into_iter().min()
    }
}
