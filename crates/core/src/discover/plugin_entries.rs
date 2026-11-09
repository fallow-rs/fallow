//! Plugin-derived entry points: compile each active plugin's entry and support
//! globs once, match them against the discovered files, and sort each hit into
//! the runtime, test or support role.

use std::path::{Path, PathBuf};

use fallow_config::{EntryPointRole, ResolvedConfig};
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
use fallow_types::path_util::is_absolute_path_any_platform;
use rustc_hash::{FxHashMap, FxHashSet};

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
/// compiling the glob index once, and matching it against every discovered file.
pub struct TimedPluginEntryPoints {
    pub entries: CategorizedEntryPoints,
    /// Compiling plugin patterns into a glob index plus per-glob rules. Scales
    /// with the number of active plugin patterns, not with project size.
    pub build_ms: f64,
    /// Matching the compiled index against every discovered file. Scales with
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
    let (glob_index, glob_meta) = build_plugin_glob_meta(plugin_result, production_skip.as_ref());
    let build_ms = build_start.elapsed().as_secs_f64() * 1000.0;

    let match_start = std::time::Instant::now();
    if !glob_index.is_empty() {
        match_plugin_entry_files(
            &mut entries,
            &glob_index,
            &glob_meta,
            &relative_paths,
            files,
        );
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
        .filter(|plugin| plugin.has_explicit_support_role())
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

/// Compile plugin entry-pattern and support globs into a glob index plus per-glob metadata.
fn build_plugin_glob_meta<'a>(
    plugin_result: &'a crate::plugins::AggregatedPluginResult,
    production_skip: Option<&FxHashSet<&str>>,
) -> (PluginGlobIndex, Vec<CompiledEntryRule<'a>>) {
    let mut globs: Vec<(String, globset::Glob)> = Vec::new();
    let mut glob_meta: Vec<CompiledEntryRule<'a>> = Vec::new();
    for (rule, pname) in plugin_result.unique_entry_patterns() {
        if let Some((include, mut compiled)) = compile_entry_rule(rule, pname, plugin_result) {
            compiled.production_excluded = compiled.role == EntryPointRole::Support
                && production_skip.is_some_and(|names| names.contains(pname.as_str()));
            globs.push((rule.pattern.clone(), include));
            glob_meta.push(compiled);
        }
    }
    for (pattern, pname) in plugin_result
        .discovered_always_used
        .iter()
        .chain(plugin_result.always_used.iter())
        .chain(plugin_result.fixture_patterns.iter())
    {
        let Ok(glob) = fallow_config::compile_path_glob(pattern) else {
            continue;
        };
        let Some(path) = crate::plugins::CompiledPathRule::for_entry_rule(
            &crate::plugins::PathRule::new(pattern.clone()),
            "support entry pattern",
        ) else {
            continue;
        };
        globs.push((pattern.clone(), glob));
        glob_meta.push(CompiledEntryRule {
            path,
            plugin_name: pname,
            role: EntryPointRole::Support,
            production_excluded: false,
        });
    }
    (PluginGlobIndex::build(globs), glob_meta)
}

/// Plugin entry globs, grouped by the literal directory prefix of each glob.
///
/// Plugins of a workspace package give globs with the package directory as
/// prefix. One glob set for all globs of a large monorepo exceeds the NFA size
/// limit of `globset`, and then no plugin entry point applies. A set per
/// prefix stays small, and a file is matched only against the groups of its
/// ancestor directories.
#[derive(Default)]
struct PluginGlobIndex {
    groups: FxHashMap<String, Vec<GlobChunk>>,
}

/// One compiled glob set and the positions of its globs in the rule list.
struct GlobChunk {
    set: globset::GlobSet,
    rule_indexes: Vec<usize>,
}

impl PluginGlobIndex {
    fn build(globs: Vec<(String, globset::Glob)>) -> Self {
        let mut grouped: FxHashMap<String, Vec<(usize, globset::Glob)>> = FxHashMap::default();
        for (idx, (pattern, glob)) in globs.into_iter().enumerate() {
            grouped
                .entry(literal_dir_prefix(&pattern).to_string())
                .or_default()
                .push((idx, glob));
        }
        let mut index = Self::default();
        for (prefix, members) in grouped {
            let mut chunks = Vec::new();
            build_glob_chunks(&members, &mut chunks);
            if !chunks.is_empty() {
                index.groups.insert(prefix, chunks);
            }
        }
        index
    }

    fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Positions of the globs that match `rel`, in rule order.
    ///
    /// On Windows, `rel` can contain `\` separators. `globset` matches them as
    /// `/`, so the ancestor prefixes must also be found at `\`.
    fn matches(&self, rel: &str) -> Vec<usize> {
        let normalized;
        let rel = if rel.contains(|c: char| c != '/' && std::path::is_separator(c)) {
            normalized = rel.replace(std::path::is_separator, "/");
            normalized.as_str()
        } else {
            rel
        };
        let mut hits = Vec::new();
        let mut collect = |prefix: &str| {
            if let Some(chunks) = self.groups.get(prefix) {
                for chunk in chunks {
                    for hit in chunk.set.matches(rel) {
                        hits.push(chunk.rule_indexes[hit]);
                    }
                }
            }
        };
        collect("");
        for (pos, byte) in rel.bytes().enumerate() {
            if byte == b'/' {
                collect(&rel[..pos]);
            }
        }
        hits.sort_unstable();
        hits
    }
}

/// Compile `members` into one glob set, or split them in halves until each
/// part compiles. A glob that does not compile alone is dropped with a warning.
fn build_glob_chunks(members: &[(usize, globset::Glob)], chunks: &mut Vec<GlobChunk>) {
    if members.is_empty() {
        return;
    }
    let mut builder = globset::GlobSetBuilder::new();
    for (_, glob) in members {
        builder.add(glob.clone());
    }
    match builder.build() {
        Ok(set) => chunks.push(GlobChunk {
            set,
            rule_indexes: members.iter().map(|(idx, _)| *idx).collect(),
        }),
        Err(err) if members.len() == 1 => {
            tracing::warn!(
                "plugin entry pattern '{}' could not be compiled: {err}",
                members[0].1.glob()
            );
        }
        Err(_) => {
            let (left, right) = members.split_at(members.len() / 2);
            build_glob_chunks(left, chunks);
            build_glob_chunks(right, chunks);
        }
    }
}

/// The leading directories of `pattern` that contain no glob syntax, such as
/// `packages/app` for `packages/app/src/**/*.ts`. A file can match the pattern
/// only when this prefix is one of its ancestor directories.
fn literal_dir_prefix(pattern: &str) -> &str {
    const GLOB_META: &[char] = &['*', '?', '[', ']', '{', '}', '\\'];
    let mut start = 0;
    let mut end = 0;
    for (pos, _) in pattern.match_indices('/') {
        let segment = &pattern[start..pos];
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains(GLOB_META) {
            break;
        }
        end = pos;
        start = pos + 1;
    }
    &pattern[..end]
}

/// Match each file against the compiled plugin glob index, categorizing hits by role.
fn match_plugin_entry_files(
    entries: &mut CategorizedEntryPoints,
    glob_index: &PluginGlobIndex,
    glob_meta: &[CompiledEntryRule<'_>],
    relative_paths: &[String],
    files: &[DiscoveredFile],
) {
    for (idx, rel) in relative_paths.iter().enumerate() {
        let mut matches = glob_index.matches(rel);
        matches.retain(|match_idx| glob_meta[*match_idx].matches(rel));
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use fallow_config::{EntryPointRole, FallowConfig, OutputFormat, ResolvedConfig};
    use fallow_types::discover::{DiscoveredFile, FileId};

    use super::{PluginGlobIndex, discover_plugin_entry_point_sets, literal_dir_prefix};
    use crate::plugins::{AggregatedPluginResult, PathRule};

    /// Enough workspace packages to make one glob set for all of them exceed
    /// the NFA size limit of `globset`.
    const MANY_WORKSPACES: u32 = 200;

    /// The entry globs that the Astro, Vite, Vitest, ESLint, Prettier and
    /// Tailwind plugins give for one workspace package.
    const WORKSPACE_PATTERNS: &[&str] = &[
        "src/pages/**/*.{astro,ts,tsx,js,jsx,mts,mjs,cts,cjs,md,mdx}",
        "src/layouts/**/*.astro",
        "src/content/**/*.{ts,js,mts,mjs,cts,cjs,md,mdx}",
        "src/middleware.{js,ts,mjs,mts,cjs,cts}",
        "src/middleware/index.{js,ts,mjs,mts,cjs,cts}",
        "src/actions/index.{js,ts,mjs,mts,cjs,cts}",
        "src/main.{ts,tsx,js,jsx}",
        "src/index.{ts,tsx,js,jsx}",
        "index.html",
        "**/*.{test,spec}.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
        "**/__tests__/**/*.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
        "**/*.{bench,benchmark}.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
        "astro.config.{ts,js,mjs}",
        "src/content/config.{js,ts,mjs,mts,cjs,cts}",
        "src/content.config.{js,ts,mjs,mts,cjs,cts}",
        "vite.config.{ts,js,mts,mjs}",
        "vite.config.*.{ts,js,mts,mjs}",
        "vitest.config.{ts,js,mts,mjs,cts,cjs}",
        "vitest.setup.{ts,js}",
        "vitest.workspace.{ts,js}",
        "**/src/setupTests.{ts,tsx,js,jsx}",
        "**/src/test-setup.{ts,tsx,js,jsx}",
        "eslint.config.{js,mjs,cjs,ts,mts,cts}",
        ".eslintrc.{js,cjs,mjs,json,yaml,yml}",
        ".prettierrc.{js,cjs,mjs,json,yaml,yml}",
        "prettier.config.{js,mjs,cjs,ts}",
        ".lintstagedrc.{js,cjs,mjs,json}",
        "lint-staged.config.{js,mjs,cjs}",
        "tailwind.config.{ts,js,cjs,mjs}",
        "**/__fixtures__/**/*.{ts,tsx,js,jsx,json}",
        "**/fixtures/**/*.{ts,tsx,js,jsx,json}",
    ];

    fn config_for(root: &Path) -> ResolvedConfig {
        FallowConfig::default().resolve(
            root.to_path_buf(),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        )
    }

    #[test]
    fn plugin_entries_apply_with_many_workspaces() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let config = config_for(root);

        let mut plugin_result = AggregatedPluginResult::default();
        plugin_result
            .entry_point_roles
            .insert("vitest".to_string(), EntryPointRole::Test);
        let mut files = Vec::new();
        for ws in 0..MANY_WORKSPACES {
            for pattern in WORKSPACE_PATTERNS {
                plugin_result.entry_patterns.push((
                    PathRule::new(format!("packages/p{ws}/{pattern}")),
                    "vitest".to_string(),
                ));
            }
            files.push(DiscoveredFile {
                id: FileId(ws),
                path: root.join(format!("packages/p{ws}/src/a.test.ts")),
                size_bytes: 1,
            });
        }

        let entries = discover_plugin_entry_point_sets(&plugin_result, &config, &files);

        assert_eq!(entries.test.len(), files.len());
        assert!(entries.runtime.is_empty());
    }

    #[test]
    fn glob_index_splits_a_prefix_group_that_is_too_large() {
        let globs: Vec<(String, globset::Glob)> = (0..MANY_WORKSPACES)
            .flat_map(|ws| {
                WORKSPACE_PATTERNS
                    .iter()
                    .map(move |pattern| format!("{{packages,apps}}/p{ws}/{pattern}"))
            })
            .map(|pattern| {
                let glob = crate::plugins::entry_rule_glob(&pattern).expect("valid glob");
                (pattern, glob)
            })
            .collect();

        let index = PluginGlobIndex::build(globs);

        assert!(index.groups[""].len() > 1, "the group is split");
        for ws in 0..MANY_WORKSPACES {
            let path = format!("packages/p{ws}/src/a.test.ts");
            assert!(!index.matches(&path).is_empty(), "no match for {path}");
        }
    }

    #[test]
    fn plugin_entry_name_comes_from_the_first_matching_rule() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let config = config_for(root);

        let mut plugin_result = AggregatedPluginResult::default();
        plugin_result.entry_patterns.push((
            PathRule::new("packages/app/**/*.test.ts"),
            "first".to_string(),
        ));
        plugin_result
            .entry_patterns
            .push((PathRule::new("**/*.test.ts"), "second".to_string()));
        let files = vec![DiscoveredFile {
            id: FileId(0),
            path: root.join("packages/app/src/a.test.ts"),
            size_bytes: 1,
        }];

        let entries = discover_plugin_entry_point_sets(&plugin_result, &config, &files);

        assert_eq!(entries.all.len(), 1);
        assert!(matches!(
            &entries.all[0].source,
            fallow_types::discover::EntryPointSource::Plugin { name } if name == "first"
        ));
    }

    #[test]
    fn literal_dir_prefix_stops_at_glob_syntax() {
        assert_eq!(
            literal_dir_prefix("packages/app/src/**/*.ts"),
            "packages/app/src"
        );
        assert_eq!(
            literal_dir_prefix("packages/app/index.html"),
            "packages/app"
        );
        assert_eq!(literal_dir_prefix("packages/{a,b}/index.ts"), "packages");
        assert_eq!(literal_dir_prefix("**/*.test.ts"), "");
        assert_eq!(literal_dir_prefix("index.ts"), "");
        assert_eq!(literal_dir_prefix("./src/index.ts"), "");
        assert_eq!(literal_dir_prefix("/abs/index.ts"), "");
        assert_eq!(literal_dir_prefix("src/\\*.ts"), "src");
    }

    #[test]
    fn glob_index_matches_like_one_glob_set() {
        let mut patterns: Vec<String> = vec![
            "**/*.test.ts".to_string(),
            "src/index.ts".to_string(),
            "{packages,apps}/*/src/main.ts".to_string(),
            "packages/*/vite.config.ts".to_string(),
        ];
        for ws in ["packages/a", "packages/a/nested", "apps/b"] {
            patterns.extend(WORKSPACE_PATTERNS.iter().map(|p| format!("{ws}/{p}")));
        }
        let paths = [
            "src/index.ts",
            "src/a.test.ts",
            "packages/a/src/a.test.ts",
            "packages/a/src/main.ts",
            "packages/a/vite.config.ts",
            "packages/a/index.html",
            "packages/a/nested/src/pages/x.astro",
            "packages/a/nested/eslint.config.js",
            "packages/ab/src/index.ts",
            "apps/b/src/pages/index.astro",
            "apps/b/src/main.ts",
            "apps/b/fixtures/data.json",
            "apps/c/src/index.ts",
        ];
        let globs: Vec<(String, globset::Glob)> = patterns
            .iter()
            .map(|p| {
                (
                    p.clone(),
                    crate::plugins::entry_rule_glob(p).expect("valid glob"),
                )
            })
            .collect();
        let mut builder = globset::GlobSetBuilder::new();
        for (_, glob) in &globs {
            builder.add(glob.clone());
        }
        let single = builder.build().expect("small set builds");
        let index = PluginGlobIndex::build(globs);

        for path in paths {
            let mut expected = single.matches(path);
            expected.sort_unstable();
            assert_eq!(index.matches(path), expected, "matches for {path}");
            // On Windows, the walker gives relative paths with `\` separators.
            let native = path.replace('/', std::path::MAIN_SEPARATOR_STR);
            let mut expected = single.matches(&native);
            expected.sort_unstable();
            assert_eq!(index.matches(&native), expected, "matches for {native}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn glob_index_matches_backslash_paths() {
        let globs: Vec<(String, globset::Glob)> =
            ["packages/app/**/*.test.ts", "src/main.{ts,tsx}"]
                .iter()
                .map(|p| {
                    (
                        (*p).to_string(),
                        crate::plugins::entry_rule_glob(p).expect("valid glob"),
                    )
                })
                .collect();
        let index = PluginGlobIndex::build(globs);

        assert_eq!(index.matches(r"packages\app\src\a.test.ts"), vec![0]);
        assert_eq!(index.matches(r"src\main.tsx"), vec![1]);
    }
}
