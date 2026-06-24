//! Code duplication / clone detection module.
//!
//! This module implements suffix array + LCP based clone detection
//! for TypeScript/JavaScript source files. It supports multiple detection
//! modes from strict (exact matches only) to semantic (structure-aware
//! matching that ignores identifier names and literal values).

mod cache;
pub mod deepdive;
pub mod detect;
pub mod families;
pub mod normalize;
mod shingle_filter;
pub mod token_types;
mod token_visitor;
pub mod tokenize;
pub(crate) mod types;

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use globset::{Glob, GlobSet, GlobSetBuilder};
use rayon::prelude::*;
use rustc_hash::FxHashSet;

use cache::{TokenCache, TokenCacheEntry, TokenCacheMode};
pub use deepdive::{
    CloneFingerprintKey, CloneFingerprintSet, FINGERPRINT_PREFIX, clone_fingerprint,
    dominant_identifier, fingerprint_for_fragment, group_refactoring_suggestion,
};
use detect::CloneDetector;
use normalize::normalize_and_hash_resolved;
use tokenize::{tokenize_file, tokenize_file_cross_language};
pub use types::{
    CloneFamily, CloneGroup, CloneInstance, DefaultIgnoreSkipCount, DefaultIgnoreSkips,
    DetectionMode, DuplicatesConfig, DuplicationReport, DuplicationStats, MirroredDirectory,
    RefactoringKind, RefactoringSuggestion,
};

use crate::discover::{self, DiscoveredFile};
use crate::suppress::{self, IssueKind, Suppression};

/// Built-in duplicates ignores for generated framework, tool, and test output.
///
/// These are engine policy defaults, not config-file defaults: `duplicates.ignore`
/// stays empty in round-tripped configs, while the analyzer merges these patterns
/// unless `duplicates.ignoreDefaults` is set to `false`.
pub const DUPES_DEFAULT_IGNORES: &[&str] = &[
    "**/.next/**",
    "**/.nuxt/**",
    "**/.svelte-kit/**",
    "**/.turbo/**",
    "**/.parcel-cache/**",
    "**/.vite/**",
    "**/.cache/**",
    "**/out/**",
    "**/storybook-static/**",
    "**/*.test.*",
    "**/*.spec.*",
    "**/__tests__/**",
    "**/__mocks__/**",
];

#[derive(Clone)]
pub(super) struct TokenizedFile {
    path: PathBuf,
    hashed_tokens: Vec<normalize::HashedToken>,
    file_tokens: tokenize::FileTokens,
    metadata: Option<std::fs::Metadata>,
    cache_hit: bool,
    suppressions: Vec<Suppression>,
}

struct IgnoreSet {
    all: GlobSet,
    defaults: Vec<&'static str>,
}

enum IgnoreMatch {
    Default(usize),
    User,
}

impl IgnoreSet {
    fn match_path(&self, path: &Path, matches: &mut Vec<usize>) -> Option<IgnoreMatch> {
        self.all.matches_into(path, matches);
        let first = matches.first().copied()?;
        if first < self.defaults.len() {
            Some(IgnoreMatch::Default(first))
        } else {
            Some(IgnoreMatch::User)
        }
    }
}

struct DuplicationRun {
    report: DuplicationReport,
    default_ignore_skips: DefaultIgnoreSkips,
}

struct DuplicationTokenizeContext<'a> {
    root: &'a Path,
    config: &'a DuplicatesConfig,
    extra_ignores: Option<&'a IgnoreSet>,
    default_skip_counts: &'a [AtomicUsize],
    token_cache: Option<&'a TokenCache>,
    token_cache_mode: TokenCacheMode,
    normalization: fallow_config::ResolvedNormalization,
    strip_types: bool,
    skip_imports: bool,
}

/// Run duplication detection on the given files.
///
/// This is the main entry point for the duplication analysis. It:
/// 1. Reads and tokenizes all source files in parallel
/// 2. Normalizes tokens according to the detection mode
/// 3. Runs suffix array + LCP clone detection
/// 4. Groups clone instances into families with refactoring suggestions
/// 5. Applies inline suppression filters
pub fn find_duplicates(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
) -> DuplicationReport {
    find_duplicates_inner(root, files, config, None, None).report
}

/// Run duplication detection and return human-format sidecar metadata for
/// files skipped by built-in duplicates ignores.
pub fn find_duplicates_with_default_ignore_skips(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
) -> (DuplicationReport, DefaultIgnoreSkips) {
    let run = find_duplicates_inner(root, files, config, None, None);
    (run.report, run.default_ignore_skips)
}

/// Run duplication detection with the persistent token cache enabled.
pub fn find_duplicates_cached(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    cache_root: &Path,
) -> DuplicationReport {
    find_duplicates_inner(root, files, config, None, Some(cache_root)).report
}

/// Run cached duplication detection and return human-format sidecar metadata for
/// files skipped by built-in duplicates ignores.
pub fn find_duplicates_cached_with_default_ignore_skips(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    cache_root: &Path,
) -> (DuplicationReport, DefaultIgnoreSkips) {
    let run = find_duplicates_inner(root, files, config, None, Some(cache_root));
    (run.report, run.default_ignore_skips)
}

/// Run duplication detection and only return clone groups touching `focus_files`.
///
/// This keeps all files in the matching corpus, which preserves changed-file
/// versus unchanged-file detection for diff-scoped audit runs, but avoids
/// materializing duplicate groups that cannot appear in the scoped report.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow uses FxHashSet for changed-file sets throughout analysis"
)]
pub fn find_duplicates_touching_files(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    focus_files: &FxHashSet<PathBuf>,
) -> DuplicationReport {
    find_duplicates_inner(root, files, config, Some(focus_files), None).report
}

/// Run focused duplication detection and return human-format sidecar metadata
/// for files skipped by built-in duplicates ignores.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow uses FxHashSet for changed-file sets throughout analysis"
)]
pub fn find_duplicates_touching_files_with_default_ignore_skips(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    focus_files: &FxHashSet<PathBuf>,
) -> (DuplicationReport, DefaultIgnoreSkips) {
    let run = find_duplicates_inner(root, files, config, Some(focus_files), None);
    (run.report, run.default_ignore_skips)
}

/// Run focused duplication detection with the persistent token cache enabled.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow uses FxHashSet for changed-file sets throughout analysis"
)]
pub fn find_duplicates_touching_files_cached(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    focus_files: &FxHashSet<PathBuf>,
    cache_root: &Path,
) -> DuplicationReport {
    find_duplicates_inner(root, files, config, Some(focus_files), Some(cache_root)).report
}

/// Run cached focused duplication detection and return human-format sidecar
/// metadata for files skipped by built-in duplicates ignores.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow uses FxHashSet for changed-file sets throughout analysis"
)]
pub fn find_duplicates_touching_files_cached_with_default_ignore_skips(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    focus_files: &FxHashSet<PathBuf>,
    cache_root: &Path,
) -> (DuplicationReport, DefaultIgnoreSkips) {
    let run = find_duplicates_inner(root, files, config, Some(focus_files), Some(cache_root));
    (run.report, run.default_ignore_skips)
}

/// Tokenize the corpus for duplication detection: resolves normalization and
/// cache config, tokenizes files (writing the token cache when enabled), and
/// returns the per-file token data alongside the corpus totals.
fn tokenize_corpus_for_duplicates(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    extra_ignores: Option<&IgnoreSet>,
    default_skip_counts: &[AtomicUsize],
    cache_root: Option<&Path>,
) -> (Vec<TokenizedFile>, detect::CorpusTotals) {
    let normalization =
        fallow_config::ResolvedNormalization::resolve(config.mode, &config.normalization);

    let strip_types = config.cross_language;
    let skip_imports = config.ignore_imports;

    tracing::debug!(
        ignore_imports = skip_imports,
        "duplication tokenization config"
    );

    let token_cache_mode = TokenCacheMode::new(normalization, strip_types, skip_imports);
    let cache_root = cache_root.filter(|_| files.len() >= config.min_corpus_size_for_token_cache);
    let token_cache = cache_root.map(TokenCache::load);

    let file_data = tokenize_duplication_files(
        files,
        &DuplicationTokenizeContext {
            root,
            config,
            extra_ignores,
            default_skip_counts,
            token_cache: token_cache.as_ref(),
            token_cache_mode,
            normalization,
            strip_types,
            skip_imports,
        },
    );

    if let (Some(cache_root), Some(cache)) = (cache_root, token_cache) {
        save_duplication_token_cache(cache_root, cache, files, &file_data, token_cache_mode);
    }

    tracing::info!(
        files = file_data.len(),
        "tokenized files for duplication analysis"
    );

    let corpus_totals = detect::CorpusTotals {
        files: file_data.len(),
        lines: file_data
            .iter()
            .map(|file| file.file_tokens.line_count)
            .sum(),
        tokens: file_data.iter().map(|file| file.hashed_tokens.len()).sum(),
    };
    (file_data, corpus_totals)
}

/// Run clone detection over tokenized files, then apply suppression and
/// min-occurrence filters, family grouping, mirrored-directory detection, and
/// final sorting.
fn detect_and_postprocess(
    root: &Path,
    config: &DuplicatesConfig,
    mut file_data: Vec<TokenizedFile>,
    corpus_totals: detect::CorpusTotals,
    focus_files: Option<&FxHashSet<PathBuf>>,
) -> DuplicationReport {
    if file_data.len() >= config.min_corpus_size_for_shingle_filter {
        if let Some(focus_files) = focus_files {
            shingle_filter::filter_to_focus_candidates(
                &mut file_data,
                focus_files,
                config.min_tokens,
            );
        } else {
            shingle_filter::filter_to_duplicate_candidates(&mut file_data, config.min_tokens);
        }
    }

    let suppressions_by_file: FxHashMap<PathBuf, Vec<Suppression>> = file_data
        .iter()
        .filter(|file| !file.suppressions.is_empty())
        .map(|file| (file.path.clone(), file.suppressions.clone()))
        .collect();

    let detector_data: Vec<(PathBuf, Vec<normalize::HashedToken>, tokenize::FileTokens)> =
        file_data
            .into_iter()
            .map(|file| (file.path, file.hashed_tokens, file.file_tokens))
            .collect();

    let detector = CloneDetector::new(config.min_tokens, config.min_lines, config.skip_local);
    let mut report = if let Some(focus_files) = focus_files {
        detector.detect_touching_files(detector_data, focus_files)
    } else {
        detector.detect_with_totals(detector_data, corpus_totals)
    };

    if !suppressions_by_file.is_empty() {
        apply_line_suppressions(&mut report, &suppressions_by_file);
    }

    apply_min_occurrences_filter(&mut report, config.min_occurrences);

    report.clone_families = families::group_into_families(&report.clone_groups, root);
    report.mirrored_directories =
        families::detect_mirrored_directories(&report.clone_families, root);
    report.sort();
    report
}

fn find_duplicates_inner(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    focus_files: Option<&FxHashSet<PathBuf>>,
    cache_root: Option<&Path>,
) -> DuplicationRun {
    let _span = tracing::info_span!("find_duplicates").entered();

    let extra_ignores = build_ignore_set(config);
    let default_skip_counts = extra_ignores
        .as_ref()
        .map(|ignores| {
            std::iter::repeat_with(|| AtomicUsize::new(0))
                .take(ignores.defaults.len())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let (file_data, corpus_totals) = tokenize_corpus_for_duplicates(
        root,
        files,
        config,
        extra_ignores.as_ref(),
        &default_skip_counts,
        cache_root,
    );

    let report = detect_and_postprocess(root, config, file_data, corpus_totals, focus_files);

    let default_ignore_skips =
        build_default_ignore_skips(extra_ignores.as_ref(), &default_skip_counts);

    DuplicationRun {
        report,
        default_ignore_skips,
    }
}

fn tokenize_duplication_files(
    files: &[DiscoveredFile],
    ctx: &DuplicationTokenizeContext<'_>,
) -> Vec<TokenizedFile> {
    files
        .par_iter()
        .filter_map(|file| tokenize_duplication_file(file, ctx))
        .collect()
}

fn tokenize_duplication_file(
    file: &DiscoveredFile,
    ctx: &DuplicationTokenizeContext<'_>,
) -> Option<TokenizedFile> {
    if should_skip_duplicate_file(file, ctx) {
        return None;
    }

    let metadata = std::fs::metadata(&file.path).ok()?;
    let cached_entry = ctx
        .token_cache
        .and_then(|cache| cache.get(&file.path, &metadata, ctx.token_cache_mode));
    let cache_hit = cached_entry.is_some();
    let (mut entry, suppressions) = duplication_token_cache_entry(file, ctx, cached_entry)?;
    if entry.file_tokens.tokens.is_empty() || entry.hashed_tokens.len() < ctx.config.min_tokens {
        return None;
    }

    Some(TokenizedFile {
        path: file.path.clone(),
        hashed_tokens: std::mem::take(&mut entry.hashed_tokens),
        file_tokens: entry.file_tokens,
        metadata: Some(metadata),
        cache_hit,
        suppressions,
    })
}

fn should_skip_duplicate_file(file: &DiscoveredFile, ctx: &DuplicationTokenizeContext<'_>) -> bool {
    let relative = file.path.strip_prefix(ctx.root).unwrap_or(&file.path);
    let Some(ignores) = ctx.extra_ignores else {
        return false;
    };
    let mut matches = Vec::new();
    match ignores.match_path(relative, &mut matches) {
        Some(IgnoreMatch::Default(index)) => {
            ctx.default_skip_counts[index].fetch_add(1, Ordering::Relaxed);
            true
        }
        Some(IgnoreMatch::User) => true,
        None => false,
    }
}

fn duplication_token_cache_entry(
    file: &DiscoveredFile,
    ctx: &DuplicationTokenizeContext<'_>,
    cached_entry: Option<TokenCacheEntry>,
) -> Option<(TokenCacheEntry, Vec<Suppression>)> {
    if let Some(entry) = cached_entry {
        let suppressions = entry.suppressions.clone();
        if suppress::is_file_suppressed(&suppressions, IssueKind::CodeDuplication) {
            return None;
        }
        return Some((entry, suppressions));
    }

    let source = std::fs::read_to_string(&file.path).ok()?;
    let suppressions = suppress::parse_suppressions_from_source(&source).suppressions;
    if suppress::is_file_suppressed(&suppressions, IssueKind::CodeDuplication) {
        return None;
    }
    let file_tokens = tokenize_duplication_source(file, ctx, &source);
    if file_tokens.tokens.is_empty() {
        return None;
    }
    let hashed_tokens = normalize_and_hash_resolved(&file_tokens.tokens, ctx.normalization);
    Some((
        TokenCacheEntry {
            hashed_tokens,
            file_tokens,
            suppressions: suppressions.clone(),
        },
        suppressions,
    ))
}

fn tokenize_duplication_source(
    file: &DiscoveredFile,
    ctx: &DuplicationTokenizeContext<'_>,
    source: &str,
) -> tokenize::FileTokens {
    if ctx.strip_types {
        tokenize_file_cross_language(&file.path, source, true, ctx.skip_imports)
    } else {
        tokenize_file(&file.path, source, ctx.skip_imports)
    }
}

fn save_duplication_token_cache(
    cache_root: &Path,
    mut cache: TokenCache,
    files: &[DiscoveredFile],
    file_data: &[TokenizedFile],
    mode: TokenCacheMode,
) {
    for file in file_data {
        if !file.cache_hit
            && let Some(metadata) = &file.metadata
        {
            cache.insert(
                &file.path,
                metadata,
                mode,
                &cache::TokenPayload {
                    hashed_tokens: &file.hashed_tokens,
                    file_tokens: &file.file_tokens,
                    suppressions: &file.suppressions,
                },
            );
        }
    }
    cache.retain_paths(files);
    match cache.save_if_dirty() {
        Ok(true) => {
            tracing::debug!(cache_root = %cache_root.display(), "saved duplication token cache");
        }
        Ok(false) => {
            tracing::debug!(cache_root = %cache_root.display(), "duplication token cache unchanged");
        }
        Err(err) => tracing::warn!("Failed to save duplication token cache: {err}"),
    }
}

/// Drop clone groups with fewer than `min` instances and record the count on
/// the stats block. The detector already guarantees `>= 2`, so this is a
/// no-op when `min <= 2`.
///
/// Stats split: `clone_groups` and `clone_instances` are recomputed
/// post-filter so they match the serialized array length (a CI consumer
/// reading `stats.clone_groups` and iterating `clone_groups[]` sees the same
/// count). `duplication_percentage`, `duplicated_lines`, `duplicated_tokens`,
/// and `files_with_clones` stay pre-filter so the percentage math (lines /
/// total) stays consistent and `threshold` gates / trend lines don't shift
/// when the filter changes. The hidden count is disclosed in
/// `clone_groups_below_min_occurrences`. The surviving groups feed every
/// downstream step (families, mirrored dirs, --top, baseline, changed-since,
/// workspace scoping) so there's a single source of truth.
fn apply_min_occurrences_filter(report: &mut DuplicationReport, min: usize) {
    if min <= 2 {
        return;
    }
    let before = report.clone_groups.len();
    report
        .clone_groups
        .retain(|group| group.instances.len() >= min);
    let hidden = before - report.clone_groups.len();
    if hidden == 0 {
        return;
    }
    report.stats.clone_groups_below_min_occurrences = hidden;
    report.stats.clone_groups = report.clone_groups.len();
    report.stats.clone_instances = report.clone_groups.iter().map(|g| g.instances.len()).sum();
}

/// Filter out clone instances that are suppressed by line-level comments.
#[expect(
    clippy::cast_possible_truncation,
    reason = "line numbers are bounded by source size"
)]
fn apply_line_suppressions(
    report: &mut DuplicationReport,
    suppressions_by_file: &FxHashMap<PathBuf, Vec<Suppression>>,
) {
    report.clone_groups.retain_mut(|group| {
        group.instances.retain(|instance| {
            if let Some(supps) = suppressions_by_file.get(&instance.file) {
                for line in instance.start_line..=instance.end_line {
                    if suppress::is_suppressed(supps, line as u32, IssueKind::CodeDuplication) {
                        return false;
                    }
                }
            }
            true
        });
        group.instances.len() >= 2
    });
}

/// Run duplication detection on a project directory using auto-discovered files.
///
/// This is a convenience function that handles file discovery internally.
#[must_use]
pub fn find_duplicates_in_project(root: &Path, config: &DuplicatesConfig) -> DuplicationReport {
    let resolved = crate::default_config(root);
    let files = discover::discover_files_with_plugin_scopes(&resolved);
    find_duplicates(root, &files, config)
}

/// Build a merged ignore set from built-in and user-provided duplicates ignores.
#[expect(
    clippy::expect_used,
    reason = "duplicate ignore globs are validated before clone detection"
)]
fn build_ignore_set(config: &DuplicatesConfig) -> Option<IgnoreSet> {
    if !config.ignore_defaults && config.ignore.is_empty() {
        return None;
    }

    let mut builder = GlobSetBuilder::new();
    let mut defaults = Vec::new();

    if config.ignore_defaults {
        for pattern in DUPES_DEFAULT_IGNORES {
            let glob = Glob::new(pattern).expect("default duplication ignore pattern is valid");
            defaults.push(*pattern);
            builder.add(glob);
        }
    }

    for pattern in &config.ignore {
        builder.add(
            Glob::new(pattern)
                .expect("duplicates.ignore pattern was validated at config load time"),
        );
    }

    builder.build().ok().map(|all| IgnoreSet { all, defaults })
}

fn build_default_ignore_skips(
    ignores: Option<&IgnoreSet>,
    counts: &[AtomicUsize],
) -> DefaultIgnoreSkips {
    let Some(ignores) = ignores else {
        return DefaultIgnoreSkips::default();
    };

    let by_pattern = ignores
        .defaults
        .iter()
        .zip(counts)
        .filter_map(|(pattern, count)| {
            let count = count.load(Ordering::Relaxed);
            (count > 0).then_some(DefaultIgnoreSkipCount { pattern, count })
        })
        .collect::<Vec<_>>();
    let total = by_pattern.iter().map(|entry| entry.count).sum();

    DefaultIgnoreSkips { total, by_pattern }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::FileId;

    fn ignore_set_matches(set: &IgnoreSet, path: &str) -> bool {
        let mut matches = Vec::new();
        set.match_path(Path::new(path), &mut matches).is_some()
    }

    #[test]
    fn find_duplicates_empty_files() {
        let config = DuplicatesConfig::default();
        let report = find_duplicates(Path::new("/tmp"), &[], &config);
        assert!(report.clone_groups.is_empty());
        assert!(report.clone_families.is_empty());
        assert_eq!(report.stats.total_files, 0);
    }

    #[test]
    fn build_ignore_set_empty() {
        let config = DuplicatesConfig {
            ignore_defaults: false,
            ..DuplicatesConfig::default()
        };
        assert!(build_ignore_set(&config).is_none());
    }

    #[test]
    fn build_ignore_set_valid_patterns() {
        let config = DuplicatesConfig {
            ignore_defaults: false,
            ignore: vec!["**/*.test.ts".to_string(), "**/*.spec.ts".to_string()],
            ..DuplicatesConfig::default()
        };
        let set = build_ignore_set(&config);
        assert!(set.is_some());
        let set = set.unwrap();
        assert!(ignore_set_matches(&set, "src/foo.test.ts"));
        assert!(ignore_set_matches(&set, "src/bar.spec.ts"));
        assert!(!ignore_set_matches(&set, "src/baz.ts"));
    }

    #[test]
    fn build_ignore_set_merges_defaults_with_user_patterns() {
        let config = DuplicatesConfig {
            ignore: vec!["**/foo/**".to_string()],
            ..DuplicatesConfig::default()
        };
        let set = build_ignore_set(&config).expect("ignore set");
        assert!(ignore_set_matches(&set, ".next/static/chunks/app.js"));
        assert!(ignore_set_matches(&set, "src/foo.test.ts"));
        assert!(ignore_set_matches(&set, "src/foo.spec.tsx"));
        assert!(ignore_set_matches(&set, "src/__tests__/foo.ts"));
        assert!(ignore_set_matches(&set, "src/__mocks__/foo.ts"));
        assert!(ignore_set_matches(&set, "src/foo/generated.js"));
    }

    #[test]
    fn build_ignore_set_ignore_defaults_false_uses_only_user_patterns() {
        let config = DuplicatesConfig {
            ignore_defaults: false,
            ignore: vec!["**/foo/**".to_string()],
            ..DuplicatesConfig::default()
        };
        let set = build_ignore_set(&config).expect("ignore set");
        assert!(!ignore_set_matches(&set, ".next/static/chunks/app.js"));
        assert!(!ignore_set_matches(&set, "src/foo.test.ts"));
        assert!(!ignore_set_matches(&set, "src/foo.spec.tsx"));
        assert!(!ignore_set_matches(&set, "src/__tests__/foo.ts"));
        assert!(!ignore_set_matches(&set, "src/__mocks__/foo.ts"));
        assert!(ignore_set_matches(&set, "src/foo/generated.js"));
    }

    #[test]
    fn default_ignores_skip_duplicate_test_files() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let tests_dir = dir.path().join("src").join("__tests__");
        std::fs::create_dir_all(&tests_dir).expect("create tests dir");

        let code = r#"
export function repeatedTestHelper(input: string): string {
    const trimmed = input.trim();
    const lowered = trimmed.toLowerCase();
    const compact = lowered.replaceAll(" ", "-");
    return compact;
}
"#;
        let first = tests_dir.join("first.test.ts");
        let second = tests_dir.join("second.test.ts");
        std::fs::write(&first, code).expect("write first");
        std::fs::write(&second, code).expect("write second");

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: first,
                size_bytes: code.len() as u64,
            },
            DiscoveredFile {
                id: FileId(1),
                path: second,
                size_bytes: code.len() as u64,
            },
        ];
        let config = DuplicatesConfig {
            min_tokens: 5,
            min_lines: 2,
            ..DuplicatesConfig::default()
        };

        let (report, skips) =
            find_duplicates_with_default_ignore_skips(dir.path(), &files, &config);

        assert!(report.clone_groups.is_empty());
        assert_eq!(skips.total, 2);
    }

    #[test]
    fn ignore_defaults_false_restores_duplicate_test_files() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let tests_dir = dir.path().join("src").join("__tests__");
        std::fs::create_dir_all(&tests_dir).expect("create tests dir");

        let code = r#"
export function repeatedTestHelper(input: string): string {
    const trimmed = input.trim();
    const lowered = trimmed.toLowerCase();
    const compact = lowered.replaceAll(" ", "-");
    return compact;
}
"#;
        let first = tests_dir.join("first.test.ts");
        let second = tests_dir.join("second.test.ts");
        std::fs::write(&first, code).expect("write first");
        std::fs::write(&second, code).expect("write second");

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: first,
                size_bytes: code.len() as u64,
            },
            DiscoveredFile {
                id: FileId(1),
                path: second,
                size_bytes: code.len() as u64,
            },
        ];
        let config = DuplicatesConfig {
            min_tokens: 5,
            min_lines: 2,
            ignore_defaults: false,
            ..DuplicatesConfig::default()
        };

        let report = find_duplicates(dir.path(), &files, &config);

        assert!(!report.clone_groups.is_empty());
    }

    #[test]
    fn find_duplicates_with_real_files() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src_dir = dir.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("create src dir");

        let code = r#"
export function processData(input: string): string {
    const trimmed = input.trim();
    if (trimmed.length === 0) {
        return "";
    }
    const parts = trimmed.split(",");
    const filtered = parts.filter(p => p.length > 0);
    const mapped = filtered.map(p => p.toUpperCase());
    return mapped.join(", ");
}

export function validateInput(data: string): boolean {
    if (data === null || data === undefined) {
        return false;
    }
    const cleaned = data.trim();
    if (cleaned.length < 3) {
        return false;
    }
    return true;
}
"#;

        std::fs::write(src_dir.join("original.ts"), code).expect("write original");
        std::fs::write(src_dir.join("copy.ts"), code).expect("write copy");
        std::fs::write(dir.path().join("package.json"), r#"{"name": "test"}"#)
            .expect("write package.json");

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: src_dir.join("original.ts"),
                size_bytes: code.len() as u64,
            },
            DiscoveredFile {
                id: FileId(1),
                path: src_dir.join("copy.ts"),
                size_bytes: code.len() as u64,
            },
        ];

        let config = DuplicatesConfig {
            min_tokens: 10,
            min_lines: 2,
            ..DuplicatesConfig::default()
        };

        let report = find_duplicates(dir.path(), &files, &config);
        assert!(
            !report.clone_groups.is_empty(),
            "Should detect clones in identical files"
        );
        assert!(report.stats.files_with_clones >= 2);

        assert!(
            !report.clone_families.is_empty(),
            "Should group clones into families"
        );
    }

    #[test]
    fn global_shingle_prefilter_preserves_corpus_totals() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src_dir = dir.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("create src dir");

        let duplicated = r#"
export function normalizeUser(input: string): string {
    const trimmed = input.trim();
    const lowered = trimmed.toLowerCase();
    const compact = lowered.replaceAll(" ", "-");
    return compact;
}
"#;
        let unique = r#"
export function renderInvoice(id: string): string {
    const prefix = "invoice";
    const suffix = id.padStart(6, "0");
    return `${prefix}:${suffix}`;
}
"#;

        let original_path = src_dir.join("original.ts");
        let copy_path = src_dir.join("copy.ts");
        let unique_path = src_dir.join("unique.ts");
        std::fs::write(&original_path, duplicated).expect("write original");
        std::fs::write(&copy_path, duplicated).expect("write copy");
        std::fs::write(&unique_path, unique).expect("write unique");

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: original_path,
                size_bytes: duplicated.len() as u64,
            },
            DiscoveredFile {
                id: FileId(1),
                path: copy_path,
                size_bytes: duplicated.len() as u64,
            },
            DiscoveredFile {
                id: FileId(2),
                path: unique_path,
                size_bytes: unique.len() as u64,
            },
        ];
        let config = DuplicatesConfig {
            min_tokens: 5,
            min_lines: 2,
            min_corpus_size_for_shingle_filter: 1,
            ..DuplicatesConfig::default()
        };

        let report = find_duplicates(dir.path(), &files, &config);

        assert!(!report.clone_groups.is_empty());
        assert_eq!(report.stats.total_files, 3);
        assert!(report.stats.total_tokens > report.stats.duplicated_tokens);
    }

    #[test]
    fn find_duplicates_cached_skips_token_cache_for_small_corpus() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src_dir = dir.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("create src dir");

        let code = "export function same(input: number): number {\n  const doubled = input * 2;\n  return doubled + 1;\n}\n";
        let first = src_dir.join("first.ts");
        let second = src_dir.join("second.ts");
        std::fs::write(&first, code).expect("write first");
        std::fs::write(&second, code).expect("write second");

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: first,
                size_bytes: code.len() as u64,
            },
            DiscoveredFile {
                id: FileId(1),
                path: second,
                size_bytes: code.len() as u64,
            },
        ];
        let config = DuplicatesConfig {
            min_tokens: 5,
            min_lines: 2,
            ..DuplicatesConfig::default()
        };
        let cache_root = dir.path().join(".fallow");

        let report = find_duplicates_cached(dir.path(), &files, &config, &cache_root);

        assert!(!report.clone_groups.is_empty());
        assert!(
            !cache_root.exists(),
            "small projects should avoid token-cache IO overhead"
        );
    }

    #[test]
    fn find_duplicates_touching_files_keeps_cross_corpus_matches_only_for_focus() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src_dir = dir.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("create src dir");

        let focused_code = r"
export function focused(input: number): number {
    const doubled = input * 2;
    const shifted = doubled + 10;
    return shifted / 2;
}
";
        let untouched_code = r#"
export function untouched(input: string): string {
    const lowered = input.toLowerCase();
    const padded = lowered.padStart(10, "x");
    return padded.slice(0, 8);
}
"#;

        let changed_path = src_dir.join("changed.ts");
        let focused_copy_path = src_dir.join("focused-copy.ts");
        let untouched_a_path = src_dir.join("untouched-a.ts");
        let untouched_b_path = src_dir.join("untouched-b.ts");
        std::fs::write(&changed_path, focused_code).expect("write changed");
        std::fs::write(&focused_copy_path, focused_code).expect("write focused copy");
        std::fs::write(&untouched_a_path, untouched_code).expect("write untouched a");
        std::fs::write(&untouched_b_path, untouched_code).expect("write untouched b");

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: changed_path.clone(),
                size_bytes: focused_code.len() as u64,
            },
            DiscoveredFile {
                id: FileId(1),
                path: focused_copy_path,
                size_bytes: focused_code.len() as u64,
            },
            DiscoveredFile {
                id: FileId(2),
                path: untouched_a_path,
                size_bytes: untouched_code.len() as u64,
            },
            DiscoveredFile {
                id: FileId(3),
                path: untouched_b_path,
                size_bytes: untouched_code.len() as u64,
            },
        ];

        let config = DuplicatesConfig {
            mode: DetectionMode::Strict,
            min_tokens: 5,
            min_lines: 2,
            min_corpus_size_for_shingle_filter: 1,
            ..DuplicatesConfig::default()
        };
        let mut focus = FxHashSet::default();
        focus.insert(changed_path.clone());

        let full_report = find_duplicates(dir.path(), &files, &config);
        let report = find_duplicates_touching_files(dir.path(), &files, &config, &focus);
        let expected_touching = full_report
            .clone_groups
            .iter()
            .filter(|group| {
                group
                    .instances
                    .iter()
                    .any(|instance| instance.file == changed_path)
            })
            .count();

        assert!(
            !report.clone_groups.is_empty(),
            "focused file should still match an unchanged duplicate"
        );
        assert_eq!(
            report.clone_groups.len(),
            expected_touching,
            "focused shingle filtering must not drop clone groups touching the focused file"
        );
        assert!(report.clone_groups.iter().all(|group| {
            group
                .instances
                .iter()
                .any(|instance| instance.file == changed_path)
        }));
    }

    #[test]
    fn file_wide_suppression_excludes_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src_dir = dir.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("create src dir");

        let code = r#"
export function processData(input: string): string {
    const trimmed = input.trim();
    if (trimmed.length === 0) {
        return "";
    }
    const parts = trimmed.split(",");
    const filtered = parts.filter(p => p.length > 0);
    const mapped = filtered.map(p => p.toUpperCase());
    return mapped.join(", ");
}
"#;
        let suppressed_code = format!("// fallow-ignore-file code-duplication\n{code}");

        std::fs::write(src_dir.join("original.ts"), code).expect("write original");
        std::fs::write(src_dir.join("suppressed.ts"), &suppressed_code).expect("write suppressed");
        std::fs::write(dir.path().join("package.json"), r#"{"name": "test"}"#)
            .expect("write package.json");

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: src_dir.join("original.ts"),
                size_bytes: code.len() as u64,
            },
            DiscoveredFile {
                id: FileId(1),
                path: src_dir.join("suppressed.ts"),
                size_bytes: suppressed_code.len() as u64,
            },
        ];

        let config = DuplicatesConfig {
            min_tokens: 10,
            min_lines: 2,
            ..DuplicatesConfig::default()
        };

        let report = find_duplicates(dir.path(), &files, &config);
        assert!(
            report.clone_groups.is_empty(),
            "File-wide suppression should exclude file from duplication analysis"
        );
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "test fixture; linear setup/assert, length is not a maintainability concern"
    )]
    fn min_occurrences_hides_pairs_and_records_count() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src_dir = dir.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("create src dir");

        let block_a = r#"
export function blockA(input: string): string {
    const trimmed = input.trim();
    if (trimmed.length === 0) {
        return "";
    }
    const parts = trimmed.split(",");
    const filtered = parts.filter(p => p.length > 0);
    const mapped = filtered.map(p => p.toUpperCase());
    return mapped.join(", ");
}
"#;
        let block_b = r"
export function blockB(value: number): number {
    if (value <= 0) {
        return 0;
    }
    let total = 0;
    for (let i = 1; i <= value; i += 1) {
        total += i * 2;
        total -= 1;
    }
    return total + 7;
}
";

        let pair_a1 = src_dir.join("pair-a1.ts");
        let pair_a2 = src_dir.join("pair-a2.ts");
        let triple_b1 = src_dir.join("triple-b1.ts");
        let triple_b2 = src_dir.join("triple-b2.ts");
        let triple_b3 = src_dir.join("triple-b3.ts");
        std::fs::write(&pair_a1, block_a).expect("write");
        std::fs::write(&pair_a2, block_a).expect("write");
        std::fs::write(&triple_b1, block_b).expect("write");
        std::fs::write(&triple_b2, block_b).expect("write");
        std::fs::write(&triple_b3, block_b).expect("write");

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: pair_a1,
                size_bytes: block_a.len() as u64,
            },
            DiscoveredFile {
                id: FileId(1),
                path: pair_a2,
                size_bytes: block_a.len() as u64,
            },
            DiscoveredFile {
                id: FileId(2),
                path: triple_b1,
                size_bytes: block_b.len() as u64,
            },
            DiscoveredFile {
                id: FileId(3),
                path: triple_b2,
                size_bytes: block_b.len() as u64,
            },
            DiscoveredFile {
                id: FileId(4),
                path: triple_b3,
                size_bytes: block_b.len() as u64,
            },
        ];

        let default_config = DuplicatesConfig {
            min_tokens: 10,
            min_lines: 2,
            ..DuplicatesConfig::default()
        };
        let baseline = find_duplicates(dir.path(), &files, &default_config);
        assert_eq!(
            baseline.clone_groups.len(),
            2,
            "default minOccurrences should report both the pair and the triple"
        );
        assert_eq!(
            baseline.stats.clone_groups_below_min_occurrences, 0,
            "default minOccurrences hides nothing"
        );
        let baseline_pct = baseline.stats.duplication_percentage;

        let raised_config = DuplicatesConfig {
            min_tokens: 10,
            min_lines: 2,
            min_occurrences: 3,
            ..DuplicatesConfig::default()
        };
        let report = find_duplicates(dir.path(), &files, &raised_config);
        assert_eq!(
            report.clone_groups.len(),
            1,
            "minOccurrences=3 should hide the 2-instance group"
        );
        assert_eq!(
            report.clone_groups[0].instances.len(),
            3,
            "surviving group must be the 3-instance group"
        );
        assert_eq!(
            report.stats.clone_groups_below_min_occurrences, 1,
            "the hidden 2-instance group must be counted"
        );
        assert_eq!(
            report.stats.clone_groups, 1,
            "stats.clone_groups must match the post-filter array length"
        );
        assert_eq!(
            report.stats.clone_instances, 3,
            "stats.clone_instances must match the surviving instance total"
        );
        assert!(
            (report.stats.duplication_percentage - baseline_pct).abs() < f64::EPSILON,
            "duplication_percentage should not shift when minOccurrences changes"
        );
    }

    #[test]
    fn min_occurrences_evaluates_after_line_suppressions() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src_dir = dir.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("create src dir");

        let block = r#"
export function shared(input: string): string {
    const trimmed = input.trim();
    if (trimmed.length === 0) {
        return "";
    }
    const parts = trimmed.split(",");
    const filtered = parts.filter(p => p.length > 0);
    const mapped = filtered.map(p => p.toUpperCase());
    return mapped.join(", ");
}
"#;
        let suppressed = format!("// fallow-ignore-file code-duplication\n{block}");

        let a = src_dir.join("a.ts");
        let b = src_dir.join("b.ts");
        let c = src_dir.join("c.ts");
        std::fs::write(&a, block).expect("write a");
        std::fs::write(&b, block).expect("write b");
        std::fs::write(&c, &suppressed).expect("write c");

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: a,
                size_bytes: block.len() as u64,
            },
            DiscoveredFile {
                id: FileId(1),
                path: b,
                size_bytes: block.len() as u64,
            },
            DiscoveredFile {
                id: FileId(2),
                path: c,
                size_bytes: suppressed.len() as u64,
            },
        ];

        let config = DuplicatesConfig {
            min_tokens: 10,
            min_lines: 2,
            min_occurrences: 3,
            ..DuplicatesConfig::default()
        };
        let report = find_duplicates(dir.path(), &files, &config);
        assert!(
            report.clone_groups.is_empty(),
            "post-suppression 2-instance group must be hidden by minOccurrences=3, \
             got groups: {:?}",
            report
                .clone_groups
                .iter()
                .map(|g| g.instances.len())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            report.stats.clone_groups, 0,
            "stats.clone_groups must match the empty post-filter array"
        );
        assert_eq!(
            report.stats.clone_instances, 0,
            "stats.clone_instances must match the empty post-filter array"
        );
    }
}
