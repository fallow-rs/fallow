//! Plugin-derived entry points: compile each active plugin's entry and support
//! globs once, match them against the discovered files, and sort each hit into
//! the runtime, test or support role.

use std::path::{Path, PathBuf};

use fallow_config::{EntryPointRole, ResolvedConfig};
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
use fallow_types::path_util::is_absolute_path_any_platform;
use rustc_hash::FxHashSet;

use super::super::walk::SOURCE_EXTENSIONS;
use super::{CategorizedEntryPoints, relative_paths_for};

/// Discover entry points from plugin results (dynamic config parsing).
///
/// Converts plugin-discovered patterns and setup files into concrete entry points
/// by matching them against the discovered file list.
#[must_use]
pub fn discover_plugin_entry_points(
    plugin_result: &crate::plugins::AggregatedPluginResult,
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    discover_plugin_entry_point_sets(plugin_result, config, files).all
}

/// Discover plugin-derived entry points with runtime/test/support roles preserved.
#[must_use]
pub fn discover_plugin_entry_point_sets(
    plugin_result: &crate::plugins::AggregatedPluginResult,
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> CategorizedEntryPoints {
    discover_plugin_entry_point_sets_timed(plugin_result, config, files).entries
}

/// Plugin entry discovery split into the two halves that scale differently:
/// compiling the glob set once, and matching it against every discovered file.
pub struct TimedPluginEntryPoints {
    pub entries: CategorizedEntryPoints,
    /// Compiling plugin patterns into a glob set plus per-glob rules. Scales
    /// with the number of active plugin patterns, not with project size.
    pub build_ms: f64,
    /// Matching the compiled set against every discovered file. Scales with
    /// file count times pattern count.
    pub match_ms: f64,
}

pub fn discover_plugin_entry_point_sets_timed(
    plugin_result: &crate::plugins::AggregatedPluginResult,
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> TimedPluginEntryPoints {
    let mut entries = CategorizedEntryPoints::default();

    let build_start = std::time::Instant::now();
    let relative_paths = relative_paths_for(files, &config.root);
    let production_skip = production_skipped_plugins(config);
    let (glob_set, glob_meta) = build_plugin_glob_meta(plugin_result, production_skip.as_ref());
    let build_ms = build_start.elapsed().as_secs_f64() * 1000.0;

    let match_start = std::time::Instant::now();
    if let Some(glob_set) = glob_set.filter(|set| !set.is_empty()) {
        match_plugin_entry_files(&mut entries, &glob_set, &glob_meta, &relative_paths, files);
    }
    let match_ms = match_start.elapsed().as_secs_f64() * 1000.0;

    push_plugin_setup_files(&mut entries, &config.root, plugin_result);

    TimedPluginEntryPoints {
        entries: entries.dedup(),
        build_ms,
        match_ms,
    }
}

/// Names of the user plugins (inline `framework[]` entries and plugin files)
/// whose `entryPoints` have the `support` role, in production mode only.
///
/// Production mode leaves dev-only files out of the analysis. A user support
/// entry is a dev tool by declaration, so a file that only such a rule matches
/// is left out too. Built-in plugin support entries do not change.
///
/// Entry rules carry only the plugin name. A user plugin that has the name of
/// a built-in plugin or of an internal entry source shares its rules, so such
/// a name is kept out of the set.
fn production_skipped_plugins(config: &ResolvedConfig) -> Option<FxHashSet<&str>> {
    if !config.production {
        return None;
    }
    let mut names: FxHashSet<&str> = config
        .external_plugins
        .iter()
        .filter(|plugin| plugin.entry_point_role == EntryPointRole::Support)
        .map(|plugin| plugin.name.as_str())
        .collect();
    if names.is_empty() {
        return None;
    }
    for reserved in crate::plugins::registry::builtin_plugin_names()
        .into_iter()
        .chain(INTERNAL_ENTRY_SOURCES.iter().copied())
    {
        names.remove(reserved);
    }
    (!names.is_empty()).then_some(names)
}

/// Plugin names that the script analysis gives to its own entry rules.
const INTERNAL_ENTRY_SOURCES: &[&str] = &["scripts", "script-tests"];

/// Compile plugin entry-pattern and support globs into a glob set plus per-glob metadata.
fn build_plugin_glob_meta<'a>(
    plugin_result: &'a crate::plugins::AggregatedPluginResult,
    production_skip: Option<&FxHashSet<&str>>,
) -> (Option<globset::GlobSet>, Vec<CompiledEntryRule<'a>>) {
    let mut builder = globset::GlobSetBuilder::new();
    let mut glob_meta: Vec<CompiledEntryRule<'a>> = Vec::new();
    for (rule, pname) in plugin_result.unique_entry_patterns() {
        if let Some((include, mut compiled)) = compile_entry_rule(rule, pname, plugin_result) {
            compiled.production_excluded = compiled.role == EntryPointRole::Support
                && production_skip.is_some_and(|names| names.contains(pname.as_str()));
            builder.add(include);
            glob_meta.push(compiled);
        }
    }
    for (pattern, pname) in plugin_result
        .discovered_always_used
        .iter()
        .chain(plugin_result.always_used.iter())
        .chain(plugin_result.fixture_patterns.iter())
    {
        if let Ok(glob) = fallow_config::compile_path_glob(pattern) {
            builder.add(glob);
            if let Some(path) = crate::plugins::CompiledPathRule::for_entry_rule(
                &crate::plugins::PathRule::new(pattern.clone()),
                "support entry pattern",
            ) {
                glob_meta.push(CompiledEntryRule {
                    path,
                    plugin_name: pname,
                    role: EntryPointRole::Support,
                    production_excluded: false,
                });
            }
        }
    }
    (
        super::super::plugin_glob_set_or_warn(builder.build()),
        glob_meta,
    )
}

/// Match each file against the compiled plugin glob set, categorizing hits by role.
fn match_plugin_entry_files(
    entries: &mut CategorizedEntryPoints,
    glob_set: &globset::GlobSet,
    glob_meta: &[CompiledEntryRule<'_>],
    relative_paths: &[String],
    files: &[DiscoveredFile],
) {
    for (idx, rel) in relative_paths.iter().enumerate() {
        let matches: Vec<usize> = glob_set
            .matches(rel)
            .into_iter()
            .filter(|match_idx| glob_meta[*match_idx].matches(rel))
            .collect();
        if matches.is_empty() {
            continue;
        }
        let name = glob_meta[matches[0]].plugin_name;
        let entry = EntryPoint {
            path: files[idx].path.clone(),
            source: EntryPointSource::Plugin {
                name: name.to_string(),
            },
        };
        categorize_plugin_match(entries, entry, glob_meta, &matches);
    }
}

/// Push a matched entry into the runtime/test/support buckets implied by its roles.
fn categorize_plugin_match(
    entries: &mut CategorizedEntryPoints,
    entry: EntryPoint,
    glob_meta: &[CompiledEntryRule<'_>],
    matches: &[usize],
) {
    if matches
        .iter()
        .all(|&match_idx| glob_meta[match_idx].production_excluded)
    {
        entries.production_excluded.push(entry.path);
        return;
    }
    let mut has_runtime = false;
    let mut has_test = false;
    let mut has_support = false;
    for &match_idx in matches {
        match glob_meta[match_idx].role {
            EntryPointRole::Runtime => has_runtime = true,
            EntryPointRole::Test => has_test = true,
            EntryPointRole::Support => has_support = true,
        }
    }

    if has_runtime {
        entries.push_runtime(entry.clone());
    }
    if has_test {
        entries.push_test(entry.clone());
    }
    if has_support || (!has_runtime && !has_test) {
        entries.push_support(entry);
    }
}

/// Resolve plugin-declared setup files (with source-extension fallback) into support entries.
fn push_plugin_setup_files(
    entries: &mut CategorizedEntryPoints,
    root: &Path,
    plugin_result: &crate::plugins::AggregatedPluginResult,
) {
    for (setup_file, pname) in &plugin_result.setup_files {
        let resolved = resolve_plugin_setup_file(root, setup_file);
        if resolved.exists() {
            entries.push_support(EntryPoint {
                path: resolved,
                source: EntryPointSource::Plugin {
                    name: pname.clone(),
                },
            });
            continue;
        }
        for ext in SOURCE_EXTENSIONS {
            let with_ext = resolved.with_extension(ext);
            if with_ext.exists() {
                entries.push_support(EntryPoint {
                    path: with_ext,
                    source: EntryPointSource::Plugin {
                        name: pname.clone(),
                    },
                });
                break;
            }
        }
    }
}

pub(super) fn resolve_plugin_setup_file(root: &Path, setup_file: &Path) -> PathBuf {
    if is_absolute_path_any_platform(setup_file) {
        setup_file.to_path_buf()
    } else {
        root.join(setup_file)
    }
}

struct CompiledEntryRule<'a> {
    path: crate::plugins::CompiledPathRule,
    plugin_name: &'a str,
    role: EntryPointRole,
    /// A user `support` entry rule in production mode. See
    /// [`production_skipped_plugins`].
    production_excluded: bool,
}

impl CompiledEntryRule<'_> {
    fn matches(&self, path: &str) -> bool {
        self.path.matches(path)
    }
}

fn compile_entry_rule<'a>(
    rule: &'a crate::plugins::PathRule,
    plugin_name: &'a str,
    plugin_result: &'a crate::plugins::AggregatedPluginResult,
) -> Option<(globset::Glob, CompiledEntryRule<'a>)> {
    let include = match crate::plugins::entry_rule_glob(&rule.pattern) {
        Ok(glob) => glob,
        Err(err) => {
            tracing::warn!("invalid entry pattern '{}': {err}", rule.pattern);
            return None;
        }
    };
    let role = plugin_result
        .entry_point_roles
        .get(plugin_name)
        .copied()
        .unwrap_or(EntryPointRole::Support);
    Some((
        include,
        CompiledEntryRule {
            path: crate::plugins::CompiledPathRule::for_entry_rule(rule, "entry pattern")?,
            plugin_name,
            role,
            production_excluded: false,
        },
    ))
}
