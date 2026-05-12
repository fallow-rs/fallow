use std::path::{Component, Path, PathBuf};

use super::parse_scripts::extract_script_file_refs;
use super::walk::SOURCE_EXTENSIONS;
use fallow_config::{EntryPointRole, PackageJson, ResolvedConfig};
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
use rustc_hash::FxHashMap;

/// Known output directory names from exports maps.
/// When an entry point path is inside one of these directories, we also try
/// the `src/` equivalent to find the tracked source file.
const OUTPUT_DIRS: &[&str] = &["dist", "build", "out", "esm", "cjs"];
const SKIPPED_ENTRY_WARNING_PREVIEW: usize = 5;

fn format_skipped_entry_warning(skipped_entries: &FxHashMap<String, usize>) -> Option<String> {
    if skipped_entries.is_empty() {
        return None;
    }

    let mut entries = skipped_entries
        .iter()
        .map(|(path, count)| (path.as_str(), *count))
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

    let preview = entries
        .iter()
        .take(SKIPPED_ENTRY_WARNING_PREVIEW)
        .map(|(path, count)| {
            if *count > 1 {
                format!("{path} ({count}x)")
            } else {
                (*path).to_owned()
            }
        })
        .collect::<Vec<_>>();

    let omitted = entries.len().saturating_sub(SKIPPED_ENTRY_WARNING_PREVIEW);
    let tail = if omitted > 0 {
        format!(" (and {omitted} more)")
    } else {
        String::new()
    };
    let total = entries.iter().map(|(_, count)| *count).sum::<usize>();
    let noun = if total == 1 {
        "package.json entry point"
    } else {
        "package.json entry points"
    };

    Some(format!(
        "Skipped {total} {noun} outside project root or containing parent directory traversal: {}{tail}",
        preview.join(", ")
    ))
}

pub fn warn_skipped_entry_summary(skipped_entries: &FxHashMap<String, usize>) {
    if let Some(message) = format_skipped_entry_warning(skipped_entries) {
        tracing::warn!("{message}");
    }
}

/// Entry points grouped by reachability role.
#[derive(Debug, Clone, Default)]
pub struct CategorizedEntryPoints {
    pub all: Vec<EntryPoint>,
    pub runtime: Vec<EntryPoint>,
    pub test: Vec<EntryPoint>,
}

impl CategorizedEntryPoints {
    pub fn push_runtime(&mut self, entry: EntryPoint) {
        self.runtime.push(entry.clone());
        self.all.push(entry);
    }

    pub fn push_test(&mut self, entry: EntryPoint) {
        self.test.push(entry.clone());
        self.all.push(entry);
    }

    pub fn push_support(&mut self, entry: EntryPoint) {
        self.all.push(entry);
    }

    pub fn extend_runtime<I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = EntryPoint>,
    {
        for entry in entries {
            self.push_runtime(entry);
        }
    }

    pub fn extend_test<I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = EntryPoint>,
    {
        for entry in entries {
            self.push_test(entry);
        }
    }

    pub fn extend_support<I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = EntryPoint>,
    {
        for entry in entries {
            self.push_support(entry);
        }
    }

    pub fn extend(&mut self, other: Self) {
        self.all.extend(other.all);
        self.runtime.extend(other.runtime);
        self.test.extend(other.test);
    }

    #[must_use]
    pub fn dedup(mut self) -> Self {
        dedup_entry_paths(&mut self.all);
        dedup_entry_paths(&mut self.runtime);
        dedup_entry_paths(&mut self.test);
        self
    }
}

fn dedup_entry_paths(entries: &mut Vec<EntryPoint>) {
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries.dedup_by(|a, b| a.path == b.path);
}

#[derive(Debug, Default)]
pub struct EntryPointDiscovery {
    pub entries: Vec<EntryPoint>,
    pub skipped_entries: FxHashMap<String, usize>,
}

/// Resolve a path relative to a base directory, with security check and extension fallback.
///
/// Returns `Some(EntryPoint)` if the path resolves to an existing file within `canonical_root`,
/// trying source extensions as fallback when the exact path doesn't exist.
/// Also handles exports map targets in output directories (e.g., `./dist/utils.js`)
/// by trying to map back to the source file (e.g., `./src/utils.ts`).
fn resolve_entry_path_with_tracking(
    base: &Path,
    entry: &str,
    canonical_root: &Path,
    source: EntryPointSource,
    mut skipped_entries: Option<&mut FxHashMap<String, usize>>,
) -> Option<EntryPoint> {
    // Wildcard exports (e.g., `./src/themes/*.css`) can't be resolved to a single
    // file. Return None and let the caller expand them separately.
    if entry.contains('*') {
        return None;
    }

    if entry_has_parent_dir(entry) {
        if let Some(skipped_entries) = skipped_entries.as_mut() {
            *skipped_entries.entry(entry.to_owned()).or_default() += 1;
        } else {
            tracing::warn!(path = %entry, "Skipping entry point containing parent directory traversal");
        }
        return None;
    }

    let resolved = base.join(entry);

    // If the path is in an output directory (dist/, build/, etc.), try mapping to src/ first.
    // This handles exports map targets like `./dist/utils.js` → `./src/utils.ts`.
    // We check this BEFORE the exists() check because even if the dist file exists,
    // fallow ignores dist/ by default, so we need the source file instead.
    if let Some(source_path) = try_output_to_source_path(base, entry) {
        return validated_entry_point(
            &source_path,
            canonical_root,
            entry,
            source,
            skipped_entries.as_deref_mut(),
        );
    }

    // When the entry lives under an output directory but has no direct src/ mirror
    // (e.g. `./dist/esm2022/index.js` where `src/esm2022/index.ts` does not exist),
    // probe the package root for a conventional source index. TypeScript libraries
    // commonly point `main`/`module`/`exports` at compiled output while keeping the
    // canonical source entry at `src/index.ts`. Without this fallback, the dist file
    // becomes the entry point, gets filtered out by the default dist ignore pattern,
    // and leaves the entire src/ tree unreachable. See issue #102.
    if is_entry_in_output_dir(entry)
        && let Some(source_path) = try_source_index_fallback(base)
    {
        tracing::info!(
            entry = %entry,
            fallback = %source_path.display(),
            "package.json entry resolves to an ignored output directory; falling back to source index"
        );
        return validated_entry_point(
            &source_path,
            canonical_root,
            entry,
            source,
            skipped_entries.as_deref_mut(),
        );
    }

    if resolved.is_file() {
        return validated_entry_point(
            &resolved,
            canonical_root,
            entry,
            source,
            skipped_entries.as_deref_mut(),
        );
    }

    // Try with source extensions
    for ext in SOURCE_EXTENSIONS {
        let with_ext = resolved.with_extension(ext);
        if with_ext.is_file() {
            return validated_entry_point(
                &with_ext,
                canonical_root,
                entry,
                source,
                skipped_entries.as_deref_mut(),
            );
        }
    }
    None
}

fn entry_has_parent_dir(entry: &str) -> bool {
    Path::new(entry)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
}

fn validated_entry_point(
    candidate: &Path,
    canonical_root: &Path,
    entry: &str,
    source: EntryPointSource,
    mut skipped_entries: Option<&mut FxHashMap<String, usize>>,
) -> Option<EntryPoint> {
    let canonical_candidate = match dunce::canonicalize(candidate) {
        Ok(path) => path,
        Err(err) => {
            tracing::warn!(
                path = %candidate.display(),
                %entry,
                error = %err,
                "Skipping entry point that could not be canonicalized"
            );
            return None;
        }
    };

    if !canonical_candidate.starts_with(canonical_root) {
        if let Some(skipped_entries) = skipped_entries.as_mut() {
            *skipped_entries.entry(entry.to_owned()).or_default() += 1;
        } else {
            tracing::warn!(
                path = %candidate.display(),
                %entry,
                "Skipping entry point outside project root"
            );
        }
        return None;
    }

    Some(EntryPoint {
        path: candidate.to_path_buf(),
        source,
    })
}

pub fn resolve_entry_path(
    base: &Path,
    entry: &str,
    canonical_root: &Path,
    source: EntryPointSource,
) -> Option<EntryPoint> {
    resolve_entry_path_with_tracking(base, entry, canonical_root, source, None)
}
/// Try to map an entry path from an output directory to its source equivalent.
///
/// Given `base=/project/packages/ui` and `entry=./dist/utils.js`, this tries:
/// - `/project/packages/ui/src/utils.ts`
/// - `/project/packages/ui/src/utils.tsx`
/// - etc. for all source extensions
///
/// Preserves any path prefix between the package root and the output dir,
/// e.g. `./modules/dist/utils.js` → `base/modules/src/utils.ts`.
///
/// Returns `Some(path)` if a source file is found.
fn try_output_to_source_path(base: &Path, entry: &str) -> Option<PathBuf> {
    let entry_path = Path::new(entry);
    let components: Vec<_> = entry_path.components().collect();

    // Find the last output directory component in the entry path
    let output_pos = components.iter().rposition(|c| {
        if let std::path::Component::Normal(s) = c
            && let Some(name) = s.to_str()
        {
            return OUTPUT_DIRS.contains(&name);
        }
        false
    })?;

    // Build the relative prefix before the output dir, filtering out CurDir (".")
    let prefix: PathBuf = components[..output_pos]
        .iter()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect();

    // Build the relative path after the output dir (e.g., "utils.js")
    let suffix: PathBuf = components[output_pos + 1..].iter().collect();

    // Try base + prefix + "src" + suffix-with-source-extension
    for ext in SOURCE_EXTENSIONS {
        let source_candidate = base
            .join(&prefix)
            .join("src")
            .join(suffix.with_extension(ext));
        if source_candidate.exists() {
            return Some(source_candidate);
        }
    }

    None
}

/// Conventional source index file stems probed when a package.json entry lives
/// in an ignored output directory. Ordered by preference.
const SOURCE_INDEX_FALLBACK_STEMS: &[&str] = &["src/index", "src/main", "index", "main"];

/// Return `true` when `entry` contains a known output directory component.
///
/// Matches any segment in `OUTPUT_DIRS`, e.g. `./dist/esm2022/index.js` →
/// `true`, `src/main.ts` → `false`.
fn is_entry_in_output_dir(entry: &str) -> bool {
    Path::new(entry).components().any(|c| {
        matches!(
            c,
            std::path::Component::Normal(s)
                if s.to_str().is_some_and(|name| OUTPUT_DIRS.contains(&name))
        )
    })
}

/// Probe a package root for a conventional source index file.
///
/// Used when `package.json` points at compiled output but the canonical source
/// entry is a standard TypeScript/JavaScript index file. Tries `src/index`,
/// `src/main`, `index`, and `main` with each supported source extension, in
/// that order.
fn try_source_index_fallback(base: &Path) -> Option<PathBuf> {
    for stem in SOURCE_INDEX_FALLBACK_STEMS {
        for ext in SOURCE_EXTENSIONS {
            let candidate = base.join(format!("{stem}.{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Default index patterns used when no other entry points are found.
const DEFAULT_INDEX_PATTERNS: &[&str] = &[
    "src/index.{ts,tsx,js,jsx}",
    "src/main.{ts,tsx,js,jsx}",
    "index.{ts,tsx,js,jsx}",
    "main.{ts,tsx,js,jsx}",
];

/// Fall back to default index patterns if no entries were found.
///
/// When `ws_filter` is `Some`, only files whose path starts with the given
/// workspace root are considered (used for workspace-scoped discovery).
fn apply_default_fallback(
    files: &[DiscoveredFile],
    root: &Path,
    ws_filter: Option<&Path>,
) -> Vec<EntryPoint> {
    let default_matchers: Vec<globset::GlobMatcher> = DEFAULT_INDEX_PATTERNS
        .iter()
        .filter_map(|p| globset::Glob::new(p).ok().map(|g| g.compile_matcher()))
        .collect();

    let mut entries = Vec::new();
    for file in files {
        // Use strip_prefix instead of canonicalize for workspace filtering
        if let Some(ws_root) = ws_filter
            && file.path.strip_prefix(ws_root).is_err()
        {
            continue;
        }
        let relative = file.path.strip_prefix(root).unwrap_or(&file.path);
        let relative_str = relative.to_string_lossy();
        if default_matchers
            .iter()
            .any(|m| m.is_match(relative_str.as_ref()))
        {
            entries.push(EntryPoint {
                path: file.path.clone(),
                source: EntryPointSource::DefaultIndex,
            });
        }
    }
    entries
}

/// Discover entry points from package.json, framework rules, and defaults.
fn discover_entry_points_with_warnings_impl(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
    root_pkg: Option<&PackageJson>,
    include_nested_package_entries: bool,
) -> EntryPointDiscovery {
    let _span = tracing::info_span!("discover_entry_points").entered();
    let mut discovery = EntryPointDiscovery::default();

    // Pre-compute relative paths for all files (once, not per pattern)
    let relative_paths: Vec<String> = files
        .iter()
        .map(|f| {
            f.path
                .strip_prefix(&config.root)
                .unwrap_or(&f.path)
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    // 1. Manual entries from config — batch all patterns into a single GlobSet
    // for O(files) matching instead of O(patterns × files).
    {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in &config.entry_patterns {
            if let Ok(glob) = globset::Glob::new(pattern) {
                builder.add(glob);
            }
        }
        if let Ok(glob_set) = builder.build()
            && !glob_set.is_empty()
        {
            for (idx, rel) in relative_paths.iter().enumerate() {
                if glob_set.is_match(rel) {
                    discovery.entries.push(EntryPoint {
                        path: files[idx].path.clone(),
                        source: EntryPointSource::ManualEntry,
                    });
                }
            }
        }
    }

    // 2. Package.json entries
    // Pre-compute canonical root once for all resolve_entry_path calls
    let canonical_root = dunce::canonicalize(&config.root).unwrap_or_else(|_| config.root.clone());
    if let Some(pkg) = root_pkg {
        for entry_path in pkg.entry_points() {
            if let Some(ep) = resolve_entry_path_with_tracking(
                &config.root,
                &entry_path,
                &canonical_root,
                EntryPointSource::PackageJsonMain,
                Some(&mut discovery.skipped_entries),
            ) {
                discovery.entries.push(ep);
            }
        }

        // 2b. Package.json scripts — extract file references as entry points
        if let Some(scripts) = &pkg.scripts {
            for script_value in scripts.values() {
                for file_ref in extract_script_file_refs(script_value) {
                    if let Some(ep) = resolve_entry_path_with_tracking(
                        &config.root,
                        &file_ref,
                        &canonical_root,
                        EntryPointSource::PackageJsonScript,
                        Some(&mut discovery.skipped_entries),
                    ) {
                        discovery.entries.push(ep);
                    }
                }
            }
        }

        // Framework rules now flow through PluginRegistry via external_plugins.
    }

    // 4. Auto-discover nested package.json entry points
    // For monorepo-like structures without explicit workspace config, scan for
    // package.json files in subdirectories and use their main/exports as entries.
    if include_nested_package_entries {
        let exports_dirs = root_pkg
            .map(PackageJson::exports_subdirectories)
            .unwrap_or_default();
        discover_nested_package_entries(
            &config.root,
            files,
            &mut discovery.entries,
            &canonical_root,
            &exports_dirs,
            &mut discovery.skipped_entries,
        );
    }

    // 5. Default index files (if no other entries found)
    if discovery.entries.is_empty() {
        discovery.entries = apply_default_fallback(files, &config.root, None);
    }

    // Deduplicate by path
    discovery.entries.sort_by(|a, b| a.path.cmp(&b.path));
    discovery.entries.dedup_by(|a, b| a.path == b.path);

    discovery
}

pub fn discover_entry_points_with_warnings_from_pkg(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
    root_pkg: Option<&PackageJson>,
    include_nested_package_entries: bool,
) -> EntryPointDiscovery {
    discover_entry_points_with_warnings_impl(
        config,
        files,
        root_pkg,
        include_nested_package_entries,
    )
}

pub fn discover_entry_points_with_warnings(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> EntryPointDiscovery {
    let pkg_path = config.root.join("package.json");
    let root_pkg = PackageJson::load(&pkg_path).ok();
    discover_entry_points_with_warnings_impl(config, files, root_pkg.as_ref(), true)
}

pub fn discover_entry_points(config: &ResolvedConfig, files: &[DiscoveredFile]) -> Vec<EntryPoint> {
    let discovery = discover_entry_points_with_warnings(config, files);
    warn_skipped_entry_summary(&discovery.skipped_entries);
    discovery.entries
}

/// Discover entry points from nested package.json files in subdirectories.
///
/// Scans two sources for sub-packages:
/// 1. Common monorepo directory patterns (`packages/`, `apps/`, `libs/`, etc.)
/// 2. Directories derived from the root package.json `exports` map keys
///    (e.g., `"./compat": {...}` implies `compat/` may be a sub-package)
///
/// For each discovered sub-package with a `package.json`, the `main`, `module`,
/// `source`, `exports`, and `bin` fields are treated as entry points.
fn discover_nested_package_entries(
    root: &Path,
    _files: &[DiscoveredFile],
    entries: &mut Vec<EntryPoint>,
    canonical_root: &Path,
    exports_subdirectories: &[String],
    skipped_entries: &mut FxHashMap<String, usize>,
) {
    let mut visited = rustc_hash::FxHashSet::default();

    // 1. Walk common monorepo patterns
    let search_dirs = [
        "packages", "apps", "libs", "modules", "plugins", "services", "tools", "utils",
    ];
    for dir_name in &search_dirs {
        let search_dir = root.join(dir_name);
        if !search_dir.is_dir() {
            continue;
        }
        let Ok(read_dir) = std::fs::read_dir(&search_dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let pkg_dir = entry.path();
            if visited.insert(pkg_dir.clone()) {
                collect_nested_package_entries(&pkg_dir, entries, canonical_root, skipped_entries);
            }
        }
    }

    // 2. Scan directories derived from the root exports map
    for dir_name in exports_subdirectories {
        let pkg_dir = root.join(dir_name);
        if pkg_dir.is_dir() && visited.insert(pkg_dir.clone()) {
            collect_nested_package_entries(&pkg_dir, entries, canonical_root, skipped_entries);
        }
    }
}

/// Collect entry points from a single sub-package directory.
fn collect_nested_package_entries(
    pkg_dir: &Path,
    entries: &mut Vec<EntryPoint>,
    canonical_root: &Path,
    skipped_entries: &mut FxHashMap<String, usize>,
) {
    let pkg_path = pkg_dir.join("package.json");
    if !pkg_path.exists() {
        return;
    }
    let Ok(pkg) = PackageJson::load(&pkg_path) else {
        return;
    };
    for entry_path in pkg.entry_points() {
        if entry_path.contains('*') {
            expand_wildcard_entries(pkg_dir, &entry_path, canonical_root, entries);
        } else if let Some(ep) = resolve_entry_path_with_tracking(
            pkg_dir,
            &entry_path,
            canonical_root,
            EntryPointSource::PackageJsonExports,
            Some(&mut *skipped_entries),
        ) {
            entries.push(ep);
        }
    }
    if let Some(scripts) = &pkg.scripts {
        for script_value in scripts.values() {
            for file_ref in extract_script_file_refs(script_value) {
                if let Some(ep) = resolve_entry_path_with_tracking(
                    pkg_dir,
                    &file_ref,
                    canonical_root,
                    EntryPointSource::PackageJsonScript,
                    Some(&mut *skipped_entries),
                ) {
                    entries.push(ep);
                }
            }
        }
    }
}

/// Expand wildcard subpath exports to matching files on disk.
///
/// Handles patterns like `./src/themes/*.css` from package.json exports maps
/// (`"./themes/*": { "import": "./src/themes/*.css" }`). Expands the `*` to
/// match actual files in the target directory.
fn expand_wildcard_entries(
    base: &Path,
    pattern: &str,
    canonical_root: &Path,
    entries: &mut Vec<EntryPoint>,
) {
    let full_pattern = base.join(pattern).to_string_lossy().to_string();
    let Ok(matches) = glob::glob(&full_pattern) else {
        return;
    };
    for path_result in matches {
        let Ok(path) = path_result else {
            continue;
        };
        if let Ok(canonical) = dunce::canonicalize(&path)
            && canonical.starts_with(canonical_root)
        {
            entries.push(EntryPoint {
                path,
                source: EntryPointSource::PackageJsonExports,
            });
        }
    }
}

/// Discover entry points for a workspace package.
#[must_use]
fn discover_workspace_entry_points_with_warnings_impl(
    ws_root: &Path,
    all_files: &[DiscoveredFile],
    pkg: Option<&PackageJson>,
) -> EntryPointDiscovery {
    let mut discovery = EntryPointDiscovery::default();

    if let Some(pkg) = pkg {
        let canonical_ws_root =
            dunce::canonicalize(ws_root).unwrap_or_else(|_| ws_root.to_path_buf());
        for entry_path in pkg.entry_points() {
            if entry_path.contains('*') {
                expand_wildcard_entries(
                    ws_root,
                    &entry_path,
                    &canonical_ws_root,
                    &mut discovery.entries,
                );
            } else if let Some(ep) = resolve_entry_path_with_tracking(
                ws_root,
                &entry_path,
                &canonical_ws_root,
                EntryPointSource::PackageJsonMain,
                Some(&mut discovery.skipped_entries),
            ) {
                discovery.entries.push(ep);
            }
        }

        // Scripts field — extract file references as entry points
        if let Some(scripts) = &pkg.scripts {
            for script_value in scripts.values() {
                for file_ref in extract_script_file_refs(script_value) {
                    if let Some(ep) = resolve_entry_path_with_tracking(
                        ws_root,
                        &file_ref,
                        &canonical_ws_root,
                        EntryPointSource::PackageJsonScript,
                        Some(&mut discovery.skipped_entries),
                    ) {
                        discovery.entries.push(ep);
                    }
                }
            }
        }

        // Framework rules now flow through PluginRegistry via external_plugins.
    }

    // Fall back to default index files if no entry points found for this workspace
    if discovery.entries.is_empty() {
        discovery.entries = apply_default_fallback(all_files, ws_root, Some(ws_root));
    }

    discovery.entries.sort_by(|a, b| a.path.cmp(&b.path));
    discovery.entries.dedup_by(|a, b| a.path == b.path);
    discovery
}

pub fn discover_workspace_entry_points_with_warnings_from_pkg(
    ws_root: &Path,
    all_files: &[DiscoveredFile],
    pkg: Option<&PackageJson>,
) -> EntryPointDiscovery {
    discover_workspace_entry_points_with_warnings_impl(ws_root, all_files, pkg)
}

#[must_use]
pub fn discover_workspace_entry_points_with_warnings(
    ws_root: &Path,
    _config: &ResolvedConfig,
    all_files: &[DiscoveredFile],
) -> EntryPointDiscovery {
    let pkg_path = ws_root.join("package.json");
    let pkg = PackageJson::load(&pkg_path).ok();
    discover_workspace_entry_points_with_warnings_impl(ws_root, all_files, pkg.as_ref())
}

#[must_use]
pub fn discover_workspace_entry_points(
    ws_root: &Path,
    config: &ResolvedConfig,
    all_files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    let discovery = discover_workspace_entry_points_with_warnings(ws_root, config, all_files);
    warn_skipped_entry_summary(&discovery.skipped_entries);
    discovery.entries
}

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
    let mut entries = CategorizedEntryPoints::default();

    // Pre-compute relative paths
    let relative_paths: Vec<String> = files
        .iter()
        .map(|f| {
            f.path
                .strip_prefix(&config.root)
                .unwrap_or(&f.path)
                .to_string_lossy()
                .into_owned()
        })
        .collect();

    // Match plugin entry patterns against files using a single GlobSet for
    // include globs, then filter candidate matches through any exclusions.
    let mut builder = globset::GlobSetBuilder::new();
    let mut glob_meta: Vec<CompiledEntryRule<'_>> = Vec::new();
    for (rule, pname) in &plugin_result.entry_patterns {
        if let Some((include, compiled)) = compile_entry_rule(rule, pname, plugin_result) {
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
        if let Ok(glob) = globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
        {
            builder.add(glob);
            if let Some(path) = crate::plugins::CompiledPathRule::for_entry_rule(
                &crate::plugins::PathRule::new(pattern.clone()),
                "support entry pattern",
            ) {
                glob_meta.push(CompiledEntryRule {
                    path,
                    plugin_name: pname,
                    role: EntryPointRole::Support,
                });
            }
        }
    }
    if let Ok(glob_set) = builder.build()
        && !glob_set.is_empty()
    {
        for (idx, rel) in relative_paths.iter().enumerate() {
            let matches: Vec<usize> = glob_set
                .matches(rel)
                .into_iter()
                .filter(|match_idx| glob_meta[*match_idx].matches(rel))
                .collect();
            if !matches.is_empty() {
                let name = glob_meta[matches[0]].plugin_name;
                let entry = EntryPoint {
                    path: files[idx].path.clone(),
                    source: EntryPointSource::Plugin {
                        name: name.to_string(),
                    },
                };

                let mut has_runtime = false;
                let mut has_test = false;
                let mut has_support = false;
                for match_idx in matches {
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
        }
    }

    // Add setup files (absolute paths from plugin config parsing)
    for (setup_file, pname) in &plugin_result.setup_files {
        let resolved = if setup_file.is_absolute() {
            setup_file.clone()
        } else {
            config.root.join(setup_file)
        };
        if resolved.exists() {
            entries.push_support(EntryPoint {
                path: resolved,
                source: EntryPointSource::Plugin {
                    name: pname.clone(),
                },
            });
        } else {
            // Try with extensions
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

    entries.dedup()
}

/// Discover entry points from `dynamicallyLoaded` config patterns.
///
/// Matches the configured glob patterns against the discovered file list and
/// marks matching files as entry points so they are never flagged as unused.
#[must_use]
pub fn discover_dynamically_loaded_entry_points(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    if config.dynamically_loaded.is_empty() {
        return Vec::new();
    }

    let mut builder = globset::GlobSetBuilder::new();
    for pattern in &config.dynamically_loaded {
        if let Ok(glob) = globset::Glob::new(pattern) {
            builder.add(glob);
        }
    }
    let Ok(glob_set) = builder.build() else {
        return Vec::new();
    };
    if glob_set.is_empty() {
        return Vec::new();
    }

    let mut entries = Vec::new();
    for file in files {
        let rel = file
            .path
            .strip_prefix(&config.root)
            .unwrap_or(&file.path)
            .to_string_lossy();
        if glob_set.is_match(rel.as_ref()) {
            entries.push(EntryPoint {
                path: file.path.clone(),
                source: EntryPointSource::DynamicallyLoaded,
            });
        }
    }
    entries
}

struct CompiledEntryRule<'a> {
    path: crate::plugins::CompiledPathRule,
    plugin_name: &'a str,
    role: EntryPointRole,
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
    let include = match globset::GlobBuilder::new(&rule.pattern)
        .literal_separator(true)
        .build()
    {
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
        },
    ))
}

/// Pre-compile a set of glob patterns for efficient matching against many paths.
#[must_use]
pub fn compile_glob_set(patterns: &[String]) -> Option<globset::GlobSet> {
    if patterns.is_empty() {
        return None;
    }
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
        {
            builder.add(glob);
        }
    }
    builder.build().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::{FallowConfig, OutputFormat, RulesConfig};
    use fallow_types::discover::FileId;
    use proptest::prelude::*;

    proptest! {
        /// Valid glob patterns should never panic when compiled via globset.
        #[test]
        fn glob_patterns_never_panic_on_compile(
            prefix in "[a-zA-Z0-9_]{1,20}",
            ext in prop::sample::select(vec!["ts", "tsx", "js", "jsx", "vue", "svelte", "astro", "mdx"]),
        ) {
            let pattern = format!("**/{prefix}*.{ext}");
            // Should not panic — either compiles or returns Err gracefully
            let result = globset::Glob::new(&pattern);
            prop_assert!(result.is_ok(), "Glob::new should not fail for well-formed patterns");
        }

        /// Non-source extensions should NOT be in the SOURCE_EXTENSIONS list.
        #[test]
        fn non_source_extensions_not_in_list(
            ext in prop::sample::select(vec!["py", "rb", "rs", "go", "java", "xml", "yaml", "toml", "md", "txt", "png", "jpg", "wasm", "lock"]),
        ) {
            prop_assert!(
                !SOURCE_EXTENSIONS.contains(&ext),
                "Extension '{ext}' should NOT be in SOURCE_EXTENSIONS"
            );
        }

        /// compile_glob_set should never panic on arbitrary well-formed glob patterns.
        #[test]
        fn compile_glob_set_no_panic(
            patterns in prop::collection::vec("[a-zA-Z0-9_*/.]{1,30}", 0..10),
        ) {
            // Should not panic regardless of input
            let _ = compile_glob_set(&patterns);
        }
    }

    // compile_glob_set unit tests
    #[test]
    fn compile_glob_set_empty_input() {
        assert!(
            compile_glob_set(&[]).is_none(),
            "empty patterns should return None"
        );
    }

    #[test]
    fn compile_glob_set_valid_patterns() {
        let patterns = vec!["**/*.ts".to_string(), "src/**/*.js".to_string()];
        let set = compile_glob_set(&patterns);
        assert!(set.is_some(), "valid patterns should compile");
        let set = set.unwrap();
        assert!(set.is_match("src/foo.ts"));
        assert!(set.is_match("src/bar.js"));
        assert!(!set.is_match("src/bar.py"));
    }

    #[test]
    fn compile_glob_set_keeps_star_within_a_single_path_segment() {
        let patterns = vec!["composables/*.{ts,js}".to_string()];
        let set = compile_glob_set(&patterns).expect("pattern should compile");

        assert!(set.is_match("composables/useFoo.ts"));
        assert!(!set.is_match("composables/nested/useFoo.ts"));
    }

    #[test]
    fn plugin_entry_point_sets_preserve_runtime_test_and_support_roles() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(root.join("src/runtime.ts"), "export const runtime = 1;").unwrap();
        std::fs::write(root.join("src/setup.ts"), "export const setup = 1;").unwrap();
        std::fs::write(root.join("tests/app.test.ts"), "export const test = 1;").unwrap();

        let config = FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            duplicates: fallow_config::DuplicatesConfig::default(),
            health: fallow_config::HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: fallow_config::BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            dynamically_loaded: vec![],
            overrides: vec![],
            regression: None,
            audit: fallow_config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: fallow_config::FlagsConfig::default(),
            resolve: fallow_config::ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
        }
        .resolve(root.to_path_buf(), OutputFormat::Human, 4, true, true);

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: root.join("src/runtime.ts"),
                size_bytes: 1,
            },
            DiscoveredFile {
                id: FileId(1),
                path: root.join("src/setup.ts"),
                size_bytes: 1,
            },
            DiscoveredFile {
                id: FileId(2),
                path: root.join("tests/app.test.ts"),
                size_bytes: 1,
            },
        ];

        let mut plugin_result = crate::plugins::AggregatedPluginResult::default();
        plugin_result.entry_patterns.push((
            crate::plugins::PathRule::new("src/runtime.ts"),
            "runtime-plugin".to_string(),
        ));
        plugin_result.entry_patterns.push((
            crate::plugins::PathRule::new("tests/app.test.ts"),
            "test-plugin".to_string(),
        ));
        plugin_result
            .always_used
            .push(("src/setup.ts".to_string(), "support-plugin".to_string()));
        plugin_result
            .entry_point_roles
            .insert("runtime-plugin".to_string(), EntryPointRole::Runtime);
        plugin_result
            .entry_point_roles
            .insert("test-plugin".to_string(), EntryPointRole::Test);
        plugin_result
            .entry_point_roles
            .insert("support-plugin".to_string(), EntryPointRole::Support);

        let entries = discover_plugin_entry_point_sets(&plugin_result, &config, &files);

        assert_eq!(entries.runtime.len(), 1, "expected one runtime entry");
        assert!(
            entries.runtime[0].path.ends_with("src/runtime.ts"),
            "runtime entry should stay runtime-only"
        );
        assert_eq!(entries.test.len(), 1, "expected one test entry");
        assert!(
            entries.test[0].path.ends_with("tests/app.test.ts"),
            "test entry should stay test-only"
        );
        assert_eq!(
            entries.all.len(),
            3,
            "support entries should stay in all entries"
        );
        assert!(
            entries
                .all
                .iter()
                .any(|entry| entry.path.ends_with("src/setup.ts")),
            "support entries should remain in the overall entry-point set"
        );
        assert!(
            !entries
                .runtime
                .iter()
                .any(|entry| entry.path.ends_with("src/setup.ts")),
            "support entries should not bleed into runtime reachability"
        );
        assert!(
            !entries
                .test
                .iter()
                .any(|entry| entry.path.ends_with("src/setup.ts")),
            "support entries should not bleed into test reachability"
        );
    }

    #[test]
    fn plugin_entry_point_rules_respect_exclusions() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("app/pages")).unwrap();
        std::fs::write(
            root.join("app/pages/index.tsx"),
            "export default function Page() { return null; }",
        )
        .unwrap();
        std::fs::write(
            root.join("app/pages/-helper.ts"),
            "export const helper = 1;",
        )
        .unwrap();

        let config = FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            duplicates: fallow_config::DuplicatesConfig::default(),
            health: fallow_config::HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: fallow_config::BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            dynamically_loaded: vec![],
            overrides: vec![],
            regression: None,
            audit: fallow_config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: fallow_config::FlagsConfig::default(),
            resolve: fallow_config::ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
        }
        .resolve(root.to_path_buf(), OutputFormat::Human, 4, true, true);

        let files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: root.join("app/pages/index.tsx"),
                size_bytes: 1,
            },
            DiscoveredFile {
                id: FileId(1),
                path: root.join("app/pages/-helper.ts"),
                size_bytes: 1,
            },
        ];

        let mut plugin_result = crate::plugins::AggregatedPluginResult::default();
        plugin_result.entry_patterns.push((
            crate::plugins::PathRule::new("app/pages/**/*.{ts,tsx,js,jsx}")
                .with_excluded_globs(["app/pages/**/-*", "app/pages/**/-*/**/*"]),
            "tanstack-router".to_string(),
        ));
        plugin_result
            .entry_point_roles
            .insert("tanstack-router".to_string(), EntryPointRole::Runtime);

        let entries = discover_plugin_entry_point_sets(&plugin_result, &config, &files);
        let entry_paths: Vec<_> = entries
            .all
            .iter()
            .map(|entry| {
                entry
                    .path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();

        assert!(entry_paths.contains(&"app/pages/index.tsx".to_string()));
        assert!(!entry_paths.contains(&"app/pages/-helper.ts".to_string()));
    }

    // resolve_entry_path unit tests
    mod resolve_entry_path_tests {
        use super::*;

        #[test]
        fn resolves_existing_file() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("index.ts"), "export const a = 1;").unwrap();

            let canonical = dunce::canonicalize(dir.path()).unwrap();
            let result = resolve_entry_path(
                dir.path(),
                "src/index.ts",
                &canonical,
                EntryPointSource::PackageJsonMain,
            );
            assert!(result.is_some(), "should resolve an existing file");
            assert!(result.unwrap().path.ends_with("src/index.ts"));
        }

        #[test]
        fn resolves_with_extension_fallback() {
            let dir = tempfile::tempdir().expect("create temp dir");
            // Use canonical base to avoid macOS /var → /private/var symlink mismatch
            let canonical = dunce::canonicalize(dir.path()).unwrap();
            let src = canonical.join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("index.ts"), "export const a = 1;").unwrap();

            // Provide path without extension — should try adding .ts, .tsx, etc.
            let result = resolve_entry_path(
                &canonical,
                "src/index",
                &canonical,
                EntryPointSource::PackageJsonMain,
            );
            assert!(
                result.is_some(),
                "should resolve via extension fallback when exact path doesn't exist"
            );
            let ep = result.unwrap();
            assert!(
                ep.path.to_string_lossy().contains("index.ts"),
                "should find index.ts via extension fallback"
            );
        }

        #[test]
        fn returns_none_for_nonexistent_file() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let canonical = dunce::canonicalize(dir.path()).unwrap();
            let result = resolve_entry_path(
                dir.path(),
                "does/not/exist.ts",
                &canonical,
                EntryPointSource::PackageJsonMain,
            );
            assert!(result.is_none(), "should return None for nonexistent files");
        }

        #[test]
        fn maps_dist_output_to_src() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("utils.ts"), "export const u = 1;").unwrap();

            // Also create the dist/ file to make sure it prefers src/
            let dist = dir.path().join("dist");
            std::fs::create_dir_all(&dist).unwrap();
            std::fs::write(dist.join("utils.js"), "// compiled").unwrap();

            let canonical = dunce::canonicalize(dir.path()).unwrap();
            let result = resolve_entry_path(
                dir.path(),
                "./dist/utils.js",
                &canonical,
                EntryPointSource::PackageJsonExports,
            );
            assert!(result.is_some(), "should resolve dist/ path to src/");
            let ep = result.unwrap();
            assert!(
                ep.path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .contains("src/utils.ts"),
                "should map ./dist/utils.js to src/utils.ts"
            );
        }

        #[test]
        fn maps_build_output_to_src() {
            let dir = tempfile::tempdir().expect("create temp dir");
            // Use canonical base to avoid macOS /var → /private/var symlink mismatch
            let canonical = dunce::canonicalize(dir.path()).unwrap();
            let src = canonical.join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("index.tsx"), "export default () => {};").unwrap();

            let result = resolve_entry_path(
                &canonical,
                "./build/index.js",
                &canonical,
                EntryPointSource::PackageJsonExports,
            );
            assert!(result.is_some(), "should map build/ output to src/");
            let ep = result.unwrap();
            assert!(
                ep.path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .contains("src/index.tsx"),
                "should map ./build/index.js to src/index.tsx"
            );
        }

        #[test]
        fn preserves_entry_point_source() {
            let dir = tempfile::tempdir().expect("create temp dir");
            std::fs::write(dir.path().join("index.ts"), "export const a = 1;").unwrap();

            let canonical = dunce::canonicalize(dir.path()).unwrap();
            let result = resolve_entry_path(
                dir.path(),
                "index.ts",
                &canonical,
                EntryPointSource::PackageJsonScript,
            );
            assert!(result.is_some());
            assert!(
                matches!(result.unwrap().source, EntryPointSource::PackageJsonScript),
                "should preserve the source kind"
            );
        }
        #[test]
        fn tracks_skipped_entries_without_logging_each_repeat() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let canonical = dunce::canonicalize(dir.path()).unwrap();
            let mut skipped_entries = FxHashMap::default();

            let result = resolve_entry_path_with_tracking(
                dir.path(),
                "../scripts/build.js",
                &canonical,
                EntryPointSource::PackageJsonScript,
                Some(&mut skipped_entries),
            );

            assert!(result.is_none(), "unsafe entry should be skipped");
            assert_eq!(
                skipped_entries.get("../scripts/build.js"),
                Some(&1),
                "warning tracker should count the skipped path"
            );
        }

        #[test]
        fn formats_skipped_entry_warning_with_counts() {
            let mut skipped_entries = FxHashMap::default();
            skipped_entries.insert("../../scripts/rm.mjs".to_owned(), 8);
            skipped_entries.insert("../utils/bar.js".to_owned(), 2);

            let warning =
                format_skipped_entry_warning(&skipped_entries).expect("warning should be rendered");

            assert_eq!(
                warning,
                "Skipped 10 package.json entry points outside project root or containing parent directory traversal: ../../scripts/rm.mjs (8x), ../utils/bar.js (2x)"
            );
        }

        #[test]
        fn rejects_parent_dir_escape_for_exact_file() {
            let sandbox = tempfile::tempdir().expect("create sandbox");
            let root = sandbox.path().join("project");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(
                sandbox.path().join("escape.ts"),
                "export const escape = true;",
            )
            .unwrap();

            let canonical = dunce::canonicalize(&root).unwrap();
            let result = resolve_entry_path(
                &root,
                "../escape.ts",
                &canonical,
                EntryPointSource::PackageJsonMain,
            );

            assert!(
                result.is_none(),
                "should reject exact paths that escape the root"
            );
        }

        #[test]
        fn rejects_parent_dir_escape_via_extension_fallback() {
            let sandbox = tempfile::tempdir().expect("create sandbox");
            let root = sandbox.path().join("project");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(
                sandbox.path().join("escape.ts"),
                "export const escape = true;",
            )
            .unwrap();

            let canonical = dunce::canonicalize(&root).unwrap();
            let result = resolve_entry_path(
                &root,
                "../escape",
                &canonical,
                EntryPointSource::PackageJsonMain,
            );

            assert!(
                result.is_none(),
                "should reject extension fallback paths that escape the root"
            );
        }
    }

    // try_output_to_source_path unit tests
    mod output_to_source_tests {
        use super::*;

        #[test]
        fn maps_dist_to_src_with_ts_extension() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("utils.ts"), "export const u = 1;").unwrap();

            let result = try_output_to_source_path(dir.path(), "./dist/utils.js");
            assert!(result.is_some());
            assert!(
                result
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
                    .contains("src/utils.ts")
            );
        }

        #[test]
        fn returns_none_when_no_source_file_exists() {
            let dir = tempfile::tempdir().expect("create temp dir");
            // No src/ directory at all
            let result = try_output_to_source_path(dir.path(), "./dist/missing.js");
            assert!(result.is_none());
        }

        #[test]
        fn ignores_non_output_directories() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("foo.ts"), "export const f = 1;").unwrap();

            // "lib" is not in OUTPUT_DIRS, so no mapping should occur
            let result = try_output_to_source_path(dir.path(), "./lib/foo.js");
            assert!(result.is_none());
        }

        #[test]
        fn maps_nested_output_path_preserving_prefix() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let modules_src = dir.path().join("modules").join("src");
            std::fs::create_dir_all(&modules_src).unwrap();
            std::fs::write(modules_src.join("helper.ts"), "export const h = 1;").unwrap();

            let result = try_output_to_source_path(dir.path(), "./modules/dist/helper.js");
            assert!(result.is_some());
            assert!(
                result
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
                    .contains("modules/src/helper.ts")
            );
        }
    }

    // Source index fallback unit tests (issue #102)
    mod source_index_fallback_tests {
        use super::*;

        #[test]
        fn detects_dist_entry_in_output_dir() {
            assert!(is_entry_in_output_dir("./dist/esm2022/index.js"));
            assert!(is_entry_in_output_dir("dist/index.js"));
            assert!(is_entry_in_output_dir("./build/index.js"));
            assert!(is_entry_in_output_dir("./out/main.js"));
            assert!(is_entry_in_output_dir("./esm/index.js"));
            assert!(is_entry_in_output_dir("./cjs/index.js"));
        }

        #[test]
        fn rejects_non_output_entry_paths() {
            assert!(!is_entry_in_output_dir("./src/index.ts"));
            assert!(!is_entry_in_output_dir("src/main.ts"));
            assert!(!is_entry_in_output_dir("./index.js"));
            assert!(!is_entry_in_output_dir(""));
        }

        #[test]
        fn rejects_substring_match_for_output_dir() {
            // "distro" contains "dist" as a substring but is not an output dir
            assert!(!is_entry_in_output_dir("./distro/index.js"));
            assert!(!is_entry_in_output_dir("./build-scripts/run.js"));
        }

        #[test]
        fn finds_src_index_ts() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            let index_path = src.join("index.ts");
            std::fs::write(&index_path, "export const a = 1;").unwrap();

            let result = try_source_index_fallback(dir.path());
            assert_eq!(result.as_deref(), Some(index_path.as_path()));
        }

        #[test]
        fn finds_src_index_tsx_when_ts_missing() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            let index_path = src.join("index.tsx");
            std::fs::write(&index_path, "export default 1;").unwrap();

            let result = try_source_index_fallback(dir.path());
            assert_eq!(result.as_deref(), Some(index_path.as_path()));
        }

        #[test]
        fn prefers_src_index_over_root_index() {
            // Source index fallback must prefer `src/index.*` over root-level `index.*`
            // because library conventions keep source under `src/`.
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            let src_index = src.join("index.ts");
            std::fs::write(&src_index, "export const a = 1;").unwrap();
            let root_index = dir.path().join("index.ts");
            std::fs::write(&root_index, "export const b = 2;").unwrap();

            let result = try_source_index_fallback(dir.path());
            assert_eq!(result.as_deref(), Some(src_index.as_path()));
        }

        #[test]
        fn falls_back_to_src_main() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            let main_path = src.join("main.ts");
            std::fs::write(&main_path, "export const a = 1;").unwrap();

            let result = try_source_index_fallback(dir.path());
            assert_eq!(result.as_deref(), Some(main_path.as_path()));
        }

        #[test]
        fn falls_back_to_root_index_when_no_src() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let index_path = dir.path().join("index.js");
            std::fs::write(&index_path, "module.exports = {};").unwrap();

            let result = try_source_index_fallback(dir.path());
            assert_eq!(result.as_deref(), Some(index_path.as_path()));
        }

        #[test]
        fn returns_none_when_nothing_matches() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let result = try_source_index_fallback(dir.path());
            assert!(result.is_none());
        }

        #[test]
        fn resolve_entry_path_falls_back_to_src_index_for_dist_entry() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let canonical = dunce::canonicalize(dir.path()).unwrap();

            // dist/esm2022/index.js exists but there's no src/esm2022/ mirror —
            // only src/index.ts. Without the fallback, resolve_entry_path would
            // return the dist file, which then gets filtered out by the ignore
            // pattern.
            let dist_dir = canonical.join("dist").join("esm2022");
            std::fs::create_dir_all(&dist_dir).unwrap();
            std::fs::write(dist_dir.join("index.js"), "export const x = 1;").unwrap();

            let src = canonical.join("src");
            std::fs::create_dir_all(&src).unwrap();
            let src_index = src.join("index.ts");
            std::fs::write(&src_index, "export const x = 1;").unwrap();

            let result = resolve_entry_path(
                &canonical,
                "./dist/esm2022/index.js",
                &canonical,
                EntryPointSource::PackageJsonMain,
            );
            assert!(result.is_some());
            let entry = result.unwrap();
            assert_eq!(entry.path, src_index);
        }

        #[test]
        fn resolve_entry_path_uses_direct_src_mirror_when_available() {
            // When `src/esm2022/index.ts` exists, the existing mirror logic wins
            // and the fallback should not fire.
            let dir = tempfile::tempdir().expect("create temp dir");
            let canonical = dunce::canonicalize(dir.path()).unwrap();

            let src_mirror = canonical.join("src").join("esm2022");
            std::fs::create_dir_all(&src_mirror).unwrap();
            let mirror_index = src_mirror.join("index.ts");
            std::fs::write(&mirror_index, "export const x = 1;").unwrap();

            // Also create src/index.ts to confirm the mirror wins over the fallback.
            let src_index = canonical.join("src").join("index.ts");
            std::fs::write(&src_index, "export const y = 2;").unwrap();

            let result = resolve_entry_path(
                &canonical,
                "./dist/esm2022/index.js",
                &canonical,
                EntryPointSource::PackageJsonMain,
            );
            assert_eq!(result.map(|e| e.path), Some(mirror_index));
        }
    }

    // apply_default_fallback unit tests
    mod default_fallback_tests {
        use super::*;

        #[test]
        fn finds_src_index_ts_as_fallback() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            let index_path = src.join("index.ts");
            std::fs::write(&index_path, "export const a = 1;").unwrap();

            let files = vec![DiscoveredFile {
                id: FileId(0),
                path: index_path.clone(),
                size_bytes: 20,
            }];

            let entries = apply_default_fallback(&files, dir.path(), None);
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].path, index_path);
            assert!(matches!(entries[0].source, EntryPointSource::DefaultIndex));
        }

        #[test]
        fn finds_root_index_js_as_fallback() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let index_path = dir.path().join("index.js");
            std::fs::write(&index_path, "module.exports = {};").unwrap();

            let files = vec![DiscoveredFile {
                id: FileId(0),
                path: index_path.clone(),
                size_bytes: 21,
            }];

            let entries = apply_default_fallback(&files, dir.path(), None);
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].path, index_path);
        }

        #[test]
        fn returns_empty_when_no_index_file() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let other_path = dir.path().join("src").join("utils.ts");

            let files = vec![DiscoveredFile {
                id: FileId(0),
                path: other_path,
                size_bytes: 10,
            }];

            let entries = apply_default_fallback(&files, dir.path(), None);
            assert!(
                entries.is_empty(),
                "non-index files should not match default fallback"
            );
        }

        #[test]
        fn workspace_filter_restricts_scope() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let ws_a = dir.path().join("packages").join("a").join("src");
            std::fs::create_dir_all(&ws_a).unwrap();
            let ws_b = dir.path().join("packages").join("b").join("src");
            std::fs::create_dir_all(&ws_b).unwrap();

            let index_a = ws_a.join("index.ts");
            let index_b = ws_b.join("index.ts");

            let files = vec![
                DiscoveredFile {
                    id: FileId(0),
                    path: index_a.clone(),
                    size_bytes: 10,
                },
                DiscoveredFile {
                    id: FileId(1),
                    path: index_b,
                    size_bytes: 10,
                },
            ];

            // Filter to workspace A only
            let ws_root = dir.path().join("packages").join("a");
            let entries = apply_default_fallback(&files, &ws_root, Some(&ws_root));
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].path, index_a);
        }
    }

    // expand_wildcard_entries unit tests
    mod wildcard_entry_tests {
        use super::*;

        #[test]
        fn expands_wildcard_css_entries() {
            // Wildcard subpath exports like `"./themes/*": { "import": "./src/themes/*.css" }`
            // should expand to actual CSS files on disk.
            let dir = tempfile::tempdir().expect("create temp dir");
            let themes = dir.path().join("src").join("themes");
            std::fs::create_dir_all(&themes).unwrap();
            std::fs::write(themes.join("dark.css"), ":root { --bg: #000; }").unwrap();
            std::fs::write(themes.join("light.css"), ":root { --bg: #fff; }").unwrap();

            let canonical = dunce::canonicalize(dir.path()).unwrap();
            let mut entries = Vec::new();
            expand_wildcard_entries(dir.path(), "./src/themes/*.css", &canonical, &mut entries);

            assert_eq!(entries.len(), 2, "should expand wildcard to 2 CSS files");
            let paths: Vec<String> = entries
                .iter()
                .map(|ep| ep.path.file_name().unwrap().to_string_lossy().to_string())
                .collect();
            assert!(paths.contains(&"dark.css".to_string()));
            assert!(paths.contains(&"light.css".to_string()));
            assert!(
                entries
                    .iter()
                    .all(|ep| matches!(ep.source, EntryPointSource::PackageJsonExports))
            );
        }

        #[test]
        fn wildcard_does_not_match_nonexistent_files() {
            let dir = tempfile::tempdir().expect("create temp dir");
            // No files matching the pattern
            std::fs::create_dir_all(dir.path().join("src/themes")).unwrap();

            let canonical = dunce::canonicalize(dir.path()).unwrap();
            let mut entries = Vec::new();
            expand_wildcard_entries(dir.path(), "./src/themes/*.css", &canonical, &mut entries);

            assert!(
                entries.is_empty(),
                "should return empty when no files match the wildcard"
            );
        }

        #[test]
        fn wildcard_only_matches_specified_extension() {
            // Wildcard pattern `*.css` should not match `.ts` files
            let dir = tempfile::tempdir().expect("create temp dir");
            let themes = dir.path().join("src").join("themes");
            std::fs::create_dir_all(&themes).unwrap();
            std::fs::write(themes.join("dark.css"), ":root {}").unwrap();
            std::fs::write(themes.join("index.ts"), "export {};").unwrap();

            let canonical = dunce::canonicalize(dir.path()).unwrap();
            let mut entries = Vec::new();
            expand_wildcard_entries(dir.path(), "./src/themes/*.css", &canonical, &mut entries);

            assert_eq!(entries.len(), 1, "should only match CSS files");
            assert!(
                entries[0]
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .ends_with(".css")
            );
        }
    }
}
