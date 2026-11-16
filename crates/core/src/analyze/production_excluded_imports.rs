//! Report production files that import a file the production patterns
//! excluded (issue #4498).
//!
//! An import of an excluded file resolves to [`ResolveResult::ExternalFile`],
//! because discovery gave the file no [`FileId`]. This pass matches those
//! targets against [`PRODUCTION_EXCLUDE_PATTERNS`], so it reads no excluded
//! file and runs no second analysis.

use std::path::{Path, PathBuf};

use fallow_config::{ResolvedConfig, WorkspaceDiagnostic, WorkspaceDiagnosticKind};
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
    excluded: &FxHashSet<FileId>,
) -> Vec<WorkspaceDiagnostic> {
    if !config.production {
        return Vec::new();
    }
    let Some(matcher) = PatternMatcher::new() else {
        return Vec::new();
    };
    let canonical_root = dunce::canonicalize(&config.root).ok();
    let mut by_pattern: FxHashMap<usize, Vec<String>> = FxHashMap::default();
    let mut seen: FxHashSet<&Path> = FxHashSet::default();

    for module in resolved_modules
        .iter()
        .filter(|module| !excluded.contains(&module.file_id))
    {
        for target in external_targets(module) {
            if !seen.insert(target.as_path()) || !has_source_extension(target) {
                continue;
            }
            let Some(relative) = relative_to_root(target, &config.root, canonical_root.as_deref())
            else {
                continue;
            };
            if config.ignore_patterns.is_match(relative) {
                continue;
            }
            if let Some(index) = matcher.first_match(relative) {
                by_pattern
                    .entry(index)
                    .or_default()
                    .push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    let mut entries: Vec<(usize, Vec<String>)> = by_pattern.into_iter().collect();
    entries.sort_unstable_by_key(|(index, _)| *index);
    entries
        .into_iter()
        .map(|(index, mut files)| {
            files.sort_unstable();
            diagnostic(config, matcher.patterns[index], files)
        })
        .collect()
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

fn external_targets(module: &ResolvedModule) -> impl Iterator<Item = &PathBuf> {
    module
        .resolved_imports
        .iter()
        .chain(&module.resolved_dynamic_imports)
        .map(|import| &import.target)
        .chain(module.re_exports.iter().map(|re_export| &re_export.target))
        .filter_map(|target| match target {
            ResolveResult::ExternalFile(path) => Some(path),
            _ => None,
        })
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
