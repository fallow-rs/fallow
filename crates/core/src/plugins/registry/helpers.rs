//! Helper functions for plugin registry orchestration.
//!
//! Contains pattern aggregation, external plugin processing, config file discovery,
//! config result merging, and plugin detection logic.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::{ExternalPluginDef, PackageJson, PluginDetection, UsedClassMemberRule};

use crate::discover::SOURCE_EXTENSIONS;

use super::super::{PathRule, Plugin, PluginResult, PluginUsedExportRule, UsedExportRule};
use super::{AggregatedPluginResult, PluginRegexValidationError, PluginRegexValidationErrorInput};

/// True when a config pattern names a source-extension config file living
/// directly in some directory (no path separator, no leading dot, all expanded
/// extensions are in `SOURCE_EXTENSIONS`).
///
/// Such patterns describe files that are already in the discovered file set, so
/// Phase 3a's in-memory matchers can find them after a `**/` prefix is added.
/// Callers use this to skip the corresponding filesystem fallback walk in
/// `discover_config_files`, which is the dominant cost on large monorepos.
#[must_use]
pub fn is_source_ext_root_pattern(pat: &str) -> bool {
    if pat.is_empty() || pat.contains('/') {
        return false;
    }
    for expanded in expand_brace_pattern(pat) {
        if expanded.starts_with('.') {
            return false;
        }
        let Some(ext) = std::path::Path::new(&expanded).extension() else {
            return false;
        };
        let Some(ext_str) = ext.to_str() else {
            return false;
        };
        if !SOURCE_EXTENSIONS.contains(&ext_str) {
            return false;
        }
    }
    true
}

/// Prepare a config pattern for `globset::Glob`.
#[must_use]
pub fn prepare_config_pattern(pat: &str) -> Cow<'_, str> {
    if is_source_ext_root_pattern(pat) {
        Cow::Owned(format!("**/{pat}"))
    } else {
        Cow::Borrowed(pat)
    }
}

/// Collect static patterns from a single plugin into the aggregated result.
pub fn process_static_patterns(
    plugin: &dyn Plugin,
    root: &Path,
    result: &mut AggregatedPluginResult,
) {
    let pname = plugin.name().to_string();
    result.active_plugins.push(pname.clone());
    result
        .entry_point_roles
        .insert(pname.clone(), plugin.entry_point_role());

    collect_static_plugin_rules(plugin, &pname, result);
    collect_static_plugin_metadata(plugin, root, result);
}

/// Collect a plugin's entry/used-export/class-member/config/always-used rules
/// into the aggregate, all scoped under `pname`.
fn collect_static_plugin_rules(
    plugin: &dyn Plugin,
    pname: &str,
    result: &mut AggregatedPluginResult,
) {
    for rule in plugin.entry_pattern_rules() {
        result.entry_patterns.push((rule, pname.to_string()));
    }
    for pat in plugin.config_patterns() {
        result.config_patterns.push((*pat).to_string());
    }
    for pat in plugin.always_used() {
        result
            .always_used
            .push(((*pat).to_string(), pname.to_string()));
    }
    for rule in plugin.used_export_rules() {
        result
            .used_exports
            .push(PluginUsedExportRule::new(pname.to_string(), rule));
    }
    for member in plugin.used_class_members() {
        result
            .used_class_members
            .push(UsedClassMemberRule::from(*member));
    }
    for rule in plugin.used_class_member_rules() {
        result.used_class_members.push(rule);
    }
    for pat in plugin.fixture_glob_patterns() {
        result
            .fixture_patterns
            .push(((*pat).to_string(), pname.to_string()));
    }
}

/// Collect a plugin's dependency/virtual-module/alias/auto-import metadata into
/// the aggregate.
fn collect_static_plugin_metadata(
    plugin: &dyn Plugin,
    root: &Path,
    result: &mut AggregatedPluginResult,
) {
    for dep in plugin.tooling_dependencies() {
        result.tooling_dependencies.push((*dep).to_string());
    }
    for prefix in plugin.virtual_module_prefixes() {
        result.virtual_module_prefixes.push((*prefix).to_string());
    }
    for suffix in plugin.virtual_package_suffixes() {
        result.virtual_package_suffixes.push((*suffix).to_string());
    }
    for pattern in plugin.generated_import_patterns() {
        result
            .generated_import_patterns
            .push((*pattern).to_string());
    }
    for prefix in plugin.generated_type_import_prefixes() {
        result
            .generated_type_import_prefixes
            .push((*prefix).to_string());
    }
    for (prefix, replacement) in plugin.path_aliases(root) {
        result.path_aliases.push((prefix.to_string(), replacement));
    }
    result.auto_imports.extend(plugin.auto_imports(root));
    result
        .provided_dependencies
        .extend(plugin.provided_dependencies());
}

/// Resolve package.json metadata hooks for active plugins.
pub fn process_package_json_metadata(
    active: &[&dyn Plugin],
    pkg: &PackageJson,
    root: &Path,
    result: &mut AggregatedPluginResult,
    regex_errors: &mut Vec<PluginRegexValidationError>,
) {
    for plugin in active {
        let package_referenced = plugin.package_json_referenced_dependencies(pkg, root);
        if !package_referenced.is_empty() {
            let pkg_path = root.join("package.json");
            result.package_referenced_dependencies.extend(
                package_referenced
                    .into_iter()
                    .map(|dep| (pkg_path.clone(), dep)),
            );
        }
        let plugin_result = plugin.resolve_package_json(pkg, root);
        if plugin_result.is_empty() {
            continue;
        }
        tracing::debug!(
            plugin = plugin.name(),
            deps = plugin_result.referenced_dependencies.len(),
            "resolved package.json metadata"
        );
        if let Err(mut errors) = process_config_result(plugin.name(), plugin_result, result, None) {
            regex_errors.append(&mut errors);
        }
    }
}

/// Determine whether an external plugin activates against the given project.
///
/// Shared between [`process_external_plugins`] and the collision-warning
/// helper in `registry::mod` so both paths agree on activation semantics.
pub fn is_external_plugin_active(
    ext: &ExternalPluginDef,
    all_deps: &[String],
    root: &Path,
    discovered_files: &[PathBuf],
) -> bool {
    if let Some(detection) = &ext.detection {
        let all_dep_refs: Vec<&str> = all_deps.iter().map(String::as_str).collect();
        check_plugin_detection(detection, &all_dep_refs, root, discovered_files)
    } else if !ext.enablers.is_empty() {
        ext.enablers.iter().any(|enabler| {
            if enabler.ends_with('/') {
                all_deps.iter().any(|d| d.starts_with(enabler))
            } else {
                all_deps.iter().any(|d| d == enabler)
            }
        })
    } else {
        false
    }
}

/// Process external plugin definitions, checking activation and aggregating patterns.
pub fn process_external_plugins(
    external_plugins: &[ExternalPluginDef],
    all_deps: &[String],
    root: &Path,
    discovered_files: &[PathBuf],
    result: &mut AggregatedPluginResult,
) {
    for ext in external_plugins {
        let is_active = is_external_plugin_active(ext, all_deps, root, discovered_files);
        if is_active {
            result.active_plugins.push(ext.name.clone());
            result
                .entry_point_roles
                .insert(ext.name.clone(), ext.entry_point_role);
            result.entry_patterns.extend(
                ext.entry_points
                    .iter()
                    .map(|p| (PathRule::new(p.clone()), ext.name.clone())),
            );
            if !ext.manifest_entries.is_empty() {
                result.entry_patterns.extend(
                    crate::plugins::manifest_entries::evaluate_manifest_entries(ext, root)
                        .into_iter()
                        .map(|rule| (rule, ext.name.clone())),
                );
            }
            result.config_patterns.extend(ext.config_patterns.clone());
            result.always_used.extend(
                ext.config_patterns
                    .iter()
                    .chain(ext.always_used.iter())
                    .map(|p| (p.clone(), ext.name.clone())),
            );
            result
                .tooling_dependencies
                .extend(ext.tooling_dependencies.clone());
            for ue in &ext.used_exports {
                result.used_exports.push(PluginUsedExportRule::new(
                    ext.name.clone(),
                    UsedExportRule::new(ue.pattern.clone(), ue.exports.clone()),
                ));
            }
            result
                .used_class_members
                .extend(ext.used_class_members.iter().cloned());
        }
    }
}

/// In-memory directory listing of config-candidate files (and discovered source
/// files), keyed by absolute directory, used to resolve plugin config patterns
/// without re-walking the filesystem.
///
/// Built once from the files the discovery walk already collected, so a
/// `discover_config_files` lookup that previously cost one filesystem stat per
/// `(plugin, ancestor-directory, pattern)` becomes an in-memory set lookup. The
/// walk respects `.gitignore` / `ignorePatterns` / the hidden-directory
/// allowlist, so config discovery via this index follows the same traversal
/// rules as source discovery (the raw filesystem path used in production mode
/// does not); see the crate docs / CHANGELOG for that deliberate refinement.
pub struct ConfigCandidateIndex {
    dirs: FxHashMap<PathBuf, FxHashSet<OsString>>,
}

impl ConfigCandidateIndex {
    /// Build the index from absolute file paths (discovered source files unioned
    /// with non-source config candidates). Files with no parent or no file name
    /// are skipped.
    #[must_use]
    pub fn build<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Self {
        let mut dirs: FxHashMap<PathBuf, FxHashSet<OsString>> = FxHashMap::default();
        for path in paths {
            if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
                dirs.entry(parent.to_path_buf())
                    .or_default()
                    .insert(name.to_os_string());
            }
        }
        Self { dirs }
    }

    /// Whether the directory `dir` contains a file named `name`, per the files
    /// the discovery walk collected. Used by file-based plugin activation to
    /// avoid a per-directory filesystem `read` probe.
    #[must_use]
    pub fn dir_contains(&self, dir: &Path, name: &OsStr) -> bool {
        self.dirs.get(dir).is_some_and(|names| names.contains(name))
    }

    /// Whether any directory at or below `root` contains a file named `name`,
    /// per the files the discovery walk collected. Lets a plugin activate from
    /// a sentinel file nested anywhere under `root` (e.g. a `.env.schema` in a
    /// workspace subdirectory) without a recursive filesystem walk.
    #[must_use]
    pub fn any_descendant_contains(&self, root: &Path, name: &OsStr) -> bool {
        self.dirs
            .iter()
            .any(|(dir, names)| dir.starts_with(root) && names.contains(name))
    }

    fn glob_matches_in_dir(&self, dir: &Path, matcher: &globset::GlobMatcher) -> Vec<PathBuf> {
        self.dirs.get(dir).map_or_else(Vec::new, |names| {
            names
                .iter()
                .filter(|name| matcher.is_match(Path::new(name)))
                .map(|name| dir.join(name))
                .collect()
        })
    }
}

/// Discover config files for plugins that were not matched against the
/// discovered source set.
///
/// This intentionally probes only known search roots instead of recursively
/// globbing the whole repository tree. Large monorepos often contain enormous
/// `node_modules` directories, and a full `**/project.json` walk becomes
/// pathological there. Callers should therefore pass a focused root list such
/// as the repo root, workspace roots, and ancestors of discovered source files.
///
/// When `candidate_index` is `Some` (the non-production fast path), patterns are
/// resolved against the in-memory directory index the discovery walk already
/// built, avoiding one filesystem stat per `(plugin, root, pattern)`. When it is
/// `None` (production mode), the filesystem is probed directly.
///
/// When `production_mode` is `false`, source-extension root-anchored patterns
/// (e.g., `webpack.config.{ts,js,mjs,cjs}`) are skipped because Phase 3a's
/// `**/`-prefixed matcher already finds them in the discovered source file
/// set. In production mode, the file walker excludes `*.config.*` and dotfile
/// configs, so the FS walk is still required to keep the discovery correct.
pub fn discover_config_files<'a>(
    config_matchers: &[(&'a dyn Plugin, Vec<globset::GlobMatcher>)],
    resolved_plugins: &FxHashSet<&str>,
    roots: &[&Path],
    production_mode: bool,
    candidate_index: Option<&ConfigCandidateIndex>,
) -> Vec<(PathBuf, &'a dyn Plugin)> {
    use rayon::prelude::*;
    let mut pending: Vec<(&'a dyn Plugin, &Path, String)> = Vec::new();
    for (plugin, _) in config_matchers {
        if resolved_plugins.contains(plugin.name()) {
            continue;
        }
        for root in roots {
            for pat in plugin.config_patterns() {
                if !production_mode && is_source_ext_root_pattern(pat) {
                    continue;
                }
                pending.push((*plugin, *root, pat.to_string()));
            }
        }
    }

    let hits: Vec<(PathBuf, &'a dyn Plugin)> = pending
        .par_iter()
        .flat_map_iter(|(plugin, root, pat)| {
            expand_brace_pattern(pat)
                .into_iter()
                .flat_map(|expanded| match candidate_index {
                    // A pattern under a non-allowlisted hidden directory
                    // (e.g. `.config/prisma.ts`) is never descended by the
                    // discovery walk, so it cannot be in the in-memory index;
                    // probe the filesystem for those few patterns even on the
                    // fast path so they stay discoverable.
                    Some(index) if !pattern_needs_filesystem(&expanded) => {
                        match_pattern_in_index(root, &expanded, index)
                    }
                    _ => discover_pattern_matches(root, &expanded),
                })
                .map(move |path| (path, *plugin))
                .collect::<Vec<_>>()
        })
        .collect();

    let mut seen: FxHashSet<(PathBuf, &'a str)> = FxHashSet::default();
    let mut config_files: Vec<(PathBuf, &'a dyn Plugin)> = Vec::with_capacity(hits.len());
    for (path, plugin) in hits {
        if seen.insert((path.clone(), plugin.name())) {
            config_files.push((path, plugin));
        }
    }
    config_files
}

fn pattern_has_glob(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[')
}

/// True when `pattern` has a directory component (any component before the
/// basename) that is a hidden directory NOT on the walk's traversal allowlist.
/// The discovery walk never descends such directories, so the in-memory
/// candidate index cannot contain files under them and the filesystem probe is
/// required to keep those configs (e.g. `.config/prisma.ts`) discoverable.
fn pattern_needs_filesystem(pattern: &str) -> bool {
    let mut components = pattern.split('/').peekable();
    let mut needs_fs = false;
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            break; // the basename is not a directory component
        }
        if component.starts_with('.')
            && component != "."
            && component != ".."
            && !crate::discover::is_allowed_hidden_dir(OsStr::new(component))
        {
            needs_fs = true;
            break;
        }
    }
    needs_fs
}

/// In-memory equivalent of [`discover_pattern_matches`], resolving `pattern`
/// against `index` instead of the filesystem. Mirrors that function's structure
/// arm-for-arm (plain path, `**/` strip, parent/glob split) so the two produce
/// identical hits for any file the index contains.
fn match_pattern_in_index(
    root: &Path,
    pattern: &str,
    index: &ConfigCandidateIndex,
) -> Vec<PathBuf> {
    if !pattern_has_glob(pattern) {
        let path = root.join(pattern);
        return match (path.parent(), path.file_name()) {
            (Some(dir), Some(name)) if index.dir_contains(dir, name) => vec![path],
            _ => Vec::new(),
        };
    }

    if let Some(stripped) = pattern.strip_prefix("**/") {
        return match_pattern_in_index(root, stripped, index);
    }

    let (dir, file_pattern) = match pattern.rsplit_once('/') {
        Some((parent, file_pattern)) if !pattern_has_glob(parent) => {
            (root.join(parent), file_pattern)
        }
        Some(_) => return Vec::new(),
        None => (root.to_path_buf(), pattern),
    };

    let Ok(matcher) = globset::Glob::new(file_pattern).map(|g| g.compile_matcher()) else {
        return Vec::new();
    };
    index.glob_matches_in_dir(&dir, &matcher)
}

fn discover_pattern_matches(root: &Path, pattern: &str) -> Vec<PathBuf> {
    if !pattern_has_glob(pattern) {
        let path = root.join(pattern);
        return if path.is_file() {
            vec![path]
        } else {
            Vec::new()
        };
    }

    if let Some(stripped) = pattern.strip_prefix("**/") {
        return discover_pattern_matches(root, stripped);
    }

    let (dir, file_pattern) = match pattern.rsplit_once('/') {
        Some((parent, file_pattern)) if !pattern_has_glob(parent) => {
            (root.join(parent), file_pattern)
        }
        Some(_) => return Vec::new(),
        None => (root.to_path_buf(), pattern),
    };

    scan_dir_for_pattern(&dir, file_pattern)
}

fn scan_dir_for_pattern(dir: &Path, file_pattern: &str) -> Vec<PathBuf> {
    let Ok(matcher) = globset::Glob::new(file_pattern).map(|g| g.compile_matcher()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| matcher.is_match(std::path::Path::new(name)))
        })
        .collect()
}

fn expand_brace_pattern(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(close_rel) = pattern[open + 1..].find('}') else {
        return vec![pattern.to_string()];
    };
    let close = open + 1 + close_rel;

    let prefix = &pattern[..open];
    let suffix = &pattern[close + 1..];
    let inner = &pattern[open + 1..close];
    let mut expanded = Vec::new();
    for option in inner.split(',') {
        for tail in expand_brace_pattern(suffix) {
            expanded.push(format!("{prefix}{option}{tail}"));
        }
    }
    expanded
}

/// Validate the user-supplied exclude regexes attached to a `PathRule`.
///
/// The originating plugin and source config file are surfaced so users can
/// locate every typo in one config-load error.
fn collect_path_rule_regex_errors(
    rule: &crate::plugins::PathRule,
    plugin_name: &str,
    config_path: Option<&Path>,
    rule_kind: &'static str,
    errors: &mut Vec<PluginRegexValidationError>,
) {
    for pattern in &rule.exclude_regexes {
        if let Err(source) = regex::Regex::new(pattern) {
            errors.push(PluginRegexValidationError::new(
                PluginRegexValidationErrorInput {
                    plugin_name,
                    config_path,
                    rule_kind,
                    field: "exclude_regexes",
                    rule_pattern: &rule.pattern,
                    regex_pattern: pattern,
                    source: &source,
                },
            ));
        }
    }
    for pattern in &rule.exclude_segment_regexes {
        if let Err(source) = regex::Regex::new(pattern) {
            errors.push(PluginRegexValidationError::new(
                PluginRegexValidationErrorInput {
                    plugin_name,
                    config_path,
                    rule_kind,
                    field: "exclude_segment_regexes",
                    rule_pattern: &rule.pattern,
                    regex_pattern: pattern,
                    source: &source,
                },
            ));
        }
    }
}

/// Merge a `PluginResult` from config parsing into the aggregated result.
///
/// `config_path` is the source config file the plugin parsed (when known).
/// It is only used to enrich config-load errors so users can find their typo.
/// Tests and inline package.json fallbacks may pass `None`.
pub fn process_config_result(
    plugin_name: &str,
    plugin_result: PluginResult,
    result: &mut AggregatedPluginResult,
    config_path: Option<&Path>,
) -> Result<(), Vec<PluginRegexValidationError>> {
    let mut regex_errors = Vec::new();

    for rule in &plugin_result.entry_patterns {
        collect_path_rule_regex_errors(
            rule,
            plugin_name,
            config_path,
            "entry_patterns[]",
            &mut regex_errors,
        );
    }
    for rule in &plugin_result.used_exports {
        collect_path_rule_regex_errors(
            &rule.path,
            plugin_name,
            config_path,
            "used_exports[].path",
            &mut regex_errors,
        );
    }
    if !regex_errors.is_empty() {
        return Err(regex_errors);
    }
    merge_plugin_result_fields(plugin_name, plugin_result, result);
    Ok(())
}

/// Merge a validated `PluginResult`'s payload fields into the aggregate, applying
/// the per-plugin `replace_*` semantics for entry patterns, used-export rules,
/// and path aliases.
fn merge_plugin_result_fields(
    pname: &str,
    plugin_result: PluginResult,
    result: &mut AggregatedPluginResult,
) {
    if plugin_result.replace_entry_patterns && !plugin_result.entry_patterns.is_empty() {
        result.entry_patterns.retain(|(_, name)| name != pname);
    }
    if plugin_result.replace_used_export_rules && !plugin_result.used_exports.is_empty() {
        result.used_exports.retain(|rule| rule.plugin_name != pname);
    }
    result.entry_patterns.extend(
        plugin_result
            .entry_patterns
            .into_iter()
            .map(|rule| (rule, pname.to_string())),
    );
    result.used_exports.extend(
        plugin_result
            .used_exports
            .into_iter()
            .map(|rule| PluginUsedExportRule::new(pname.to_string(), rule)),
    );
    result
        .used_class_members
        .extend(plugin_result.used_class_members);
    result
        .referenced_dependencies
        .extend(plugin_result.referenced_dependencies);
    result.discovered_always_used.extend(
        plugin_result
            .always_used_files
            .into_iter()
            .map(|p| (p, pname.to_string())),
    );
    for (prefix, replacement) in plugin_result.path_aliases {
        result
            .path_aliases
            .retain(|(existing_prefix, _)| existing_prefix != &prefix);
        result.path_aliases.push((prefix, replacement));
    }
    result.setup_files.extend(
        plugin_result
            .setup_files
            .into_iter()
            .map(|p| (p, pname.to_string())),
    );
    result.fixture_patterns.extend(
        plugin_result
            .fixture_patterns
            .into_iter()
            .map(|p| (p, pname.to_string())),
    );
    result
        .scss_include_paths
        .extend(plugin_result.scss_include_paths);
    result
        .static_dir_mappings
        .extend(plugin_result.static_dir_mappings);
    result
        .provided_dependencies
        .extend(plugin_result.provided_dependencies);
}

/// Check if a plugin already has a config file matched against discovered files.
pub fn check_has_config_file(
    plugin: &dyn Plugin,
    config_matchers: &[(&dyn Plugin, Vec<globset::GlobMatcher>)],
    relative_files: &[(PathBuf, String)],
) -> bool {
    !plugin.config_patterns().is_empty()
        && config_matchers.iter().any(|(p, matchers)| {
            p.name() == plugin.name()
                && relative_files
                    .iter()
                    .any(|(_, rel)| matchers.iter().any(|m| m.is_match(rel.as_str())))
        })
}

/// Check if a `PluginDetection` condition is satisfied.
pub fn check_plugin_detection(
    detection: &PluginDetection,
    all_deps: &[&str],
    root: &Path,
    discovered_files: &[PathBuf],
) -> bool {
    match detection {
        PluginDetection::Dependency { package } => all_deps.iter().any(|d| *d == package),
        PluginDetection::FileExists { pattern } => {
            if let Ok(matcher) = globset::Glob::new(pattern).map(|g| g.compile_matcher()) {
                for file in discovered_files {
                    let relative = file.strip_prefix(root).unwrap_or(file);
                    if matcher.is_match(relative) {
                        return true;
                    }
                }
            }
            let full_pattern = root.join(pattern).to_string_lossy().to_string();
            glob::glob(&full_pattern)
                .ok()
                .is_some_and(|mut g| g.next().is_some())
        }
        PluginDetection::All { conditions } => conditions
            .iter()
            .all(|c| check_plugin_detection(c, all_deps, root, discovered_files)),
        PluginDetection::Any { conditions } => conditions
            .iter()
            .any(|c| check_plugin_detection(c, all_deps, root, discovered_files)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_needs_filesystem_only_for_non_allowlisted_hidden_dirs() {
        // `.config` is a hidden directory not on the walk's allowlist, so a
        // config under it is never in the in-memory index and must be probed.
        assert!(pattern_needs_filesystem(".config/prisma.ts"));
        // Plain, nested-non-hidden, dotfile-basename, `**/`, and allowlisted
        // hidden-dir (`.storybook`) patterns all resolve from the index.
        assert!(!pattern_needs_filesystem("tsconfig.json"));
        assert!(!pattern_needs_filesystem("prisma/schema.prisma"));
        assert!(!pattern_needs_filesystem(".eslintrc.json"));
        assert!(!pattern_needs_filesystem("**/project.json"));
        assert!(!pattern_needs_filesystem(".storybook/main.ts"));
        assert!(!pattern_needs_filesystem("a/b/c.json"));
    }

    #[test]
    fn config_candidate_index_matches_plain_nested_and_glob_shapes() {
        let root = Path::new("/project");
        let index = ConfigCandidateIndex::build([
            Path::new("/project/tsconfig.json"),
            Path::new("/project/packages/a/tsconfig.json"),
            Path::new("/project/prisma/schema.prisma"),
            Path::new("/project/src/main.ts"),
        ]);

        // Plain basename resolved at the project root and at a nested root.
        assert_eq!(
            match_pattern_in_index(root, "tsconfig.json", &index),
            vec![PathBuf::from("/project/tsconfig.json")]
        );
        assert_eq!(
            match_pattern_in_index(Path::new("/project/packages/a"), "tsconfig.json", &index),
            vec![PathBuf::from("/project/packages/a/tsconfig.json")]
        );
        // Nested non-glob, `**/` strip, and a basename glob.
        assert_eq!(
            match_pattern_in_index(root, "prisma/schema.prisma", &index),
            vec![PathBuf::from("/project/prisma/schema.prisma")]
        );
        assert_eq!(
            match_pattern_in_index(root, "**/tsconfig.json", &index),
            vec![PathBuf::from("/project/tsconfig.json")]
        );
        assert_eq!(
            match_pattern_in_index(Path::new("/project/prisma"), "*.prisma", &index),
            vec![PathBuf::from("/project/prisma/schema.prisma")]
        );
        // A pattern present in no indexed directory yields nothing.
        assert!(match_pattern_in_index(root, "missing.json", &index).is_empty());
    }
}
