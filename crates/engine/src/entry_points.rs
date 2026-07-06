use std::path::{Component, Path, PathBuf};

use fallow_config::{EntryPointRole, PackageJson, ResolvedConfig};
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
use fallow_types::path_util::is_absolute_path_any_platform;
use regex::Regex;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover_walk::SOURCE_EXTENSIONS;

/// Known output directory names from exports maps.
/// When an entry point path is inside one of these directories, we also try
/// the `src/` equivalent to find the tracked source file.
const OUTPUT_DIRS: &[&str] = &["dist", "build", "out", "esm", "cjs"];
const SKIPPED_ENTRY_WARNING_PREVIEW: usize = 5;

fn extract_script_file_refs(script: &str) -> Vec<String> {
    let mut refs = Vec::new();

    const RUNNERS: &[&str] = &["node", "ts-node", "tsx", "babel-node"];

    for segment in script.split(&['&', '|', ';'][..]) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        let tokens = segment.split_whitespace().collect::<Vec<_>>();
        if tokens.is_empty() {
            continue;
        }

        let mut start = 0;
        if matches!(tokens.first(), Some(&"npx" | &"pnpx")) {
            start = 1;
        } else if tokens.len() >= 2 && matches!(tokens[0], "yarn" | "pnpm") && tokens[1] == "exec" {
            start = 2;
        }

        if start >= tokens.len() {
            continue;
        }

        let cmd = tokens[start];

        if RUNNERS.contains(&cmd) {
            for &token in &tokens[start + 1..] {
                if token.starts_with('-') {
                    continue;
                }
                if looks_like_file_path(token) {
                    refs.push(token.to_string());
                }
            }
        } else {
            for &token in &tokens[start..] {
                if token.starts_with('-') {
                    continue;
                }
                if looks_like_script_file(token) {
                    refs.push(token.to_string());
                }
            }
        }
    }

    refs
}

fn looks_like_file_path(token: &str) -> bool {
    if !could_be_file_path(token) {
        return false;
    }
    let extensions = [
        ".js", ".ts", ".mjs", ".cjs", ".mts", ".cts", ".jsx", ".tsx", ".gts", ".gjs",
    ];
    if extensions.iter().any(|ext| token.ends_with(ext)) {
        return true;
    }
    token.starts_with("./")
        || token.starts_with("../")
        || (token.contains('/') && !token.starts_with('@') && !token.contains("://"))
}

fn looks_like_script_file(token: &str) -> bool {
    if !could_be_file_path(token) {
        return false;
    }
    let extensions = [
        ".js", ".ts", ".mjs", ".cjs", ".mts", ".cts", ".jsx", ".tsx", ".gts", ".gjs",
    ];
    if !extensions.iter().any(|ext| token.ends_with(ext)) {
        return false;
    }
    token.contains('/') || token.starts_with("./") || token.starts_with("../")
}

fn could_be_file_path(token: &str) -> bool {
    if token.contains("${{") || (token.contains("}}") && !token.contains("{{")) {
        return false;
    }

    if token.contains('\\') {
        return false;
    }

    if let Some(open) = token.find('[') {
        let after_open = &token[open + 1..];
        let close_offset = after_open.find(']');
        if !matches!(close_offset, Some(offset) if offset > 0) {
            return false;
        }
    }

    true
}

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
    let Some(message) = format_skipped_entry_warning(skipped_entries) else {
        return;
    };
    if should_warn_skipped_entry(&message) {
        tracing::warn!("{message}");
    }
}

/// Process-wide dedupe for [`warn_skipped_entry_summary`]. Returns `true` when
/// `message` was newly inserted (caller should emit). On a poisoned mutex
/// returns `true` so over-warning beats swallowing.
fn should_warn_skipped_entry(message: &str) -> bool {
    static WARNED: std::sync::OnceLock<std::sync::Mutex<FxHashSet<String>>> =
        std::sync::OnceLock::new();
    let warned = WARNED.get_or_init(|| std::sync::Mutex::new(FxHashSet::default()));
    warned
        .lock()
        .map_or(true, |mut set| set.insert(message.to_owned()))
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
    if entry.contains('*') {
        return None;
    }

    if entry_has_parent_dir(entry) {
        record_or_warn_skipped_entry(
            skipped_entries.as_deref_mut(),
            entry,
            "Skipping entry point containing parent directory traversal",
        );
        return None;
    }

    if let OutputDirEntry::ShortCircuit(result) = resolve_entry_via_output_dir(
        base,
        entry,
        canonical_root,
        source.clone(),
        skipped_entries.as_deref_mut(),
    ) {
        return result;
    }

    resolve_entry_via_filesystem_probe(base, entry, canonical_root, source, skipped_entries)
}

/// Record a skipped entry in the dedup map, or warn when no map is tracking skips.
fn record_or_warn_skipped_entry(
    skipped_entries: Option<&mut FxHashMap<String, usize>>,
    entry: &str,
    warning: &str,
) {
    if let Some(skipped_entries) = skipped_entries {
        *skipped_entries.entry(entry.to_owned()).or_default() += 1;
    } else {
        tracing::warn!(path = %entry, "{warning}");
    }
}

/// Outcome of the output-directory mapping step.
///
/// `ShortCircuit` means an output-dir branch applied and carries the resolved
/// entry (which may itself be `None` when validation rejected the candidate);
/// `Continue` signals that filesystem probing should proceed.
enum OutputDirEntry {
    ShortCircuit(Option<EntryPoint>),
    Continue,
}

/// Map an output-directory entry back to a source file, short-circuiting resolution.
fn resolve_entry_via_output_dir(
    base: &Path,
    entry: &str,
    canonical_root: &Path,
    source: EntryPointSource,
    mut skipped_entries: Option<&mut FxHashMap<String, usize>>,
) -> OutputDirEntry {
    if let Some(source_path) = try_output_to_source_path(base, entry) {
        return OutputDirEntry::ShortCircuit(validated_entry_point(
            &source_path,
            canonical_root,
            entry,
            source,
            skipped_entries.as_deref_mut(),
        ));
    }

    if is_entry_in_output_dir(entry)
        && let Some(source_path) = try_source_index_fallback(base)
    {
        tracing::info!(
            entry = %entry,
            fallback = %source_path.display(),
            "package.json entry resolves to an ignored output directory; falling back to source index"
        );
        return OutputDirEntry::ShortCircuit(validated_entry_point(
            &source_path,
            canonical_root,
            entry,
            source,
            skipped_entries,
        ));
    }

    OutputDirEntry::Continue
}

/// Probe the filesystem for the entry: exact file, extension fallback, directory
/// index, then a package-root source-index fallback.
fn resolve_entry_via_filesystem_probe(
    base: &Path,
    entry: &str,
    canonical_root: &Path,
    source: EntryPointSource,
    mut skipped_entries: Option<&mut FxHashMap<String, usize>>,
) -> Option<EntryPoint> {
    let resolved = base.join(entry);

    if resolved.is_file() {
        return validated_entry_point(
            &resolved,
            canonical_root,
            entry,
            source,
            skipped_entries.as_deref_mut(),
        );
    }

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

    if let Some(index_entry) = try_directory_index_entry(&resolved) {
        return validated_entry_point(
            &index_entry,
            canonical_root,
            entry,
            source,
            skipped_entries.as_deref_mut(),
        );
    }

    if is_package_root_index_entry(entry)
        && let Some(source_path) = try_source_index_fallback(base)
    {
        tracing::info!(
            entry = %entry,
            fallback = %source_path.display(),
            "package.json root index entry is missing; falling back to source index"
        );
        return validated_entry_point(&source_path, canonical_root, entry, source, skipped_entries);
    }
    None
}

fn try_directory_index_entry(resolved: &Path) -> Option<PathBuf> {
    for ext in SOURCE_EXTENSIONS {
        let candidate = resolved.join(format!("index.{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn entry_has_parent_dir(entry: &str) -> bool {
    Path::new(entry)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
}

fn is_package_root_index_entry(entry: &str) -> bool {
    let mut components = Path::new(entry)
        .components()
        .filter(|component| !matches!(component, Component::CurDir));

    let Some(Component::Normal(file_name)) = components.next() else {
        return false;
    };
    if components.next().is_some() {
        return false;
    }

    file_name
        .to_str()
        .is_some_and(|name| name == "index" || name.starts_with("index."))
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

/// Try to map an entry path from an output directory to its source equivalent.
///
/// Given `base=/project/packages/ui` and `entry=./dist/utils.js`, this tries:
/// - `/project/packages/ui/src/utils.ts`
/// - `/project/packages/ui/src/utils.tsx`
/// - etc. for all source extensions
///
/// Preserves any path prefix between the package root and the output dir,
/// e.g. `./modules/dist/utils.js` maps to `base/modules/src/utils.ts`.
///
/// Returns `Some(path)` if a source file is found.
fn try_output_to_source_path(base: &Path, entry: &str) -> Option<PathBuf> {
    let entry_path = Path::new(entry);
    let components: Vec<_> = entry_path.components().collect();

    let output_pos = components.iter().rposition(|c| {
        if let std::path::Component::Normal(s) = c
            && let Some(name) = s.to_str()
        {
            return OUTPUT_DIRS.contains(&name);
        }
        false
    })?;

    let prefix: PathBuf = components[..output_pos]
        .iter()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect();

    let suffix: PathBuf = components[output_pos + 1..].iter().collect();

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
/// Matches any segment in `OUTPUT_DIRS`, e.g. `./dist/esm2022/index.js`
/// returns `true`, while `src/main.ts` returns `false`.
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

/// Compute each file's path relative to `root` as a forward-slash-lossy string.
fn relative_paths_for(files: &[DiscoveredFile], root: &Path) -> Vec<String> {
    files
        .iter()
        .map(|f| {
            f.path
                .strip_prefix(root)
                .unwrap_or(&f.path)
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// Push entries for files matching the user-configured manual entry glob patterns.
#[expect(
    clippy::expect_used,
    reason = "entry glob patterns are validated before entry point discovery"
)]
fn push_manual_entry_matches(
    entries: &mut Vec<EntryPoint>,
    config: &ResolvedConfig,
    relative_paths: &[String],
    files: &[DiscoveredFile],
) {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in &config.entry_patterns {
        builder.add(
            globset::Glob::new(pattern).expect("entry pattern was validated at config load time"),
        );
    }
    let Ok(glob_set) = builder.build() else {
        return;
    };
    if glob_set.is_empty() {
        return;
    }
    for (idx, rel) in relative_paths.iter().enumerate() {
        if glob_set.is_match(rel) {
            entries.push(EntryPoint {
                path: files[idx].path.clone(),
                source: EntryPointSource::ManualEntry,
            });
        }
    }
}

/// Push entries derived from a package.json's declared entry points and scripts.
fn push_package_json_entries(
    discovery: &mut EntryPointDiscovery,
    root: &Path,
    pkg: &PackageJson,
    canonical_root: &Path,
) {
    for entry_path in pkg.entry_points() {
        if let Some(ep) = resolve_entry_path_with_tracking(
            root,
            &entry_path,
            canonical_root,
            EntryPointSource::PackageJsonMain,
            Some(&mut discovery.skipped_entries),
        ) {
            discovery.entries.push(ep);
        }
    }

    let Some(scripts) = &pkg.scripts else {
        return;
    };
    for script_value in scripts.values() {
        for file_ref in extract_script_file_refs(script_value) {
            if let Some(ep) = resolve_entry_path_with_tracking(
                root,
                &file_ref,
                canonical_root,
                EntryPointSource::PackageJsonScript,
                Some(&mut discovery.skipped_entries),
            ) {
                discovery.entries.push(ep);
            }
        }
    }
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

    let relative_paths = relative_paths_for(files, &config.root);
    push_manual_entry_matches(&mut discovery.entries, config, &relative_paths, files);

    let canonical_root = dunce::canonicalize(&config.root).unwrap_or_else(|_| config.root.clone());
    if let Some(pkg) = root_pkg {
        push_package_json_entries(&mut discovery, &config.root, pkg, &canonical_root);
    }

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

    if discovery.entries.is_empty() {
        discovery.entries = apply_default_fallback(files, &config.root, None);
    }

    discovery.entries.sort_by(|a, b| a.path.cmp(&b.path));
    discovery.entries.dedup_by(|a, b| a.path == b.path);

    discovery
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
    }

    if discovery.entries.is_empty() {
        discovery.entries = apply_default_fallback(all_files, ws_root, Some(ws_root));
    }

    discovery.entries.sort_by(|a, b| a.path.cmp(&b.path));
    discovery.entries.dedup_by(|a, b| a.path == b.path);
    discovery
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

/// Converts plugin-discovered patterns and setup files into concrete entry points.
#[must_use]
pub fn discover_plugin_entry_points(
    plugin_result: &crate::plugins::AggregatedPluginResult,
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    let mut entries = crate::discover::CategorizedEntryPoints::default();

    let relative_paths = relative_paths_for(files, &config.root);
    let (glob_set, glob_meta) = build_plugin_glob_meta(plugin_result);
    if let Some(glob_set) = glob_set.filter(|set| !set.is_empty()) {
        match_plugin_entry_files(&mut entries, &glob_set, &glob_meta, &relative_paths, files);
    }

    push_plugin_setup_files(&mut entries, &config.root, plugin_result);

    entries.dedup().all
}

fn build_plugin_glob_meta(
    plugin_result: &crate::plugins::AggregatedPluginResult,
) -> (Option<globset::GlobSet>, Vec<CompiledEntryRule>) {
    let mut builder = globset::GlobSetBuilder::new();
    let mut glob_meta = Vec::new();
    for entry_pattern in plugin_result.entry_patterns() {
        if let Some((include, compiled)) = compile_plugin_entry_rule(&entry_pattern, plugin_result)
        {
            builder.add(include);
            glob_meta.push(compiled);
        }
    }
    for support_pattern in plugin_result.support_patterns() {
        let Ok(glob) = globset::GlobBuilder::new(&support_pattern.pattern)
            .literal_separator(true)
            .build()
        else {
            continue;
        };
        builder.add(glob);
        let rule = crate::plugins::PluginPathRule {
            pattern: support_pattern.pattern,
            exclude_globs: Vec::new(),
            exclude_regexes: Vec::new(),
            exclude_segment_regexes: Vec::new(),
        };
        if let Some(path) = CompiledPathRule::for_entry_rule(&rule, "support entry pattern") {
            glob_meta.push(CompiledEntryRule {
                path,
                plugin_name: support_pattern.plugin_name,
                role: EntryPointRole::Support,
            });
        }
    }
    (builder.build().ok(), glob_meta)
}

fn match_plugin_entry_files(
    entries: &mut crate::discover::CategorizedEntryPoints,
    glob_set: &globset::GlobSet,
    glob_meta: &[CompiledEntryRule],
    relative_paths: &[String],
    files: &[DiscoveredFile],
) {
    for (idx, rel) in relative_paths.iter().enumerate() {
        let matches = glob_set
            .matches(rel)
            .into_iter()
            .filter(|match_idx| glob_meta[*match_idx].matches(rel))
            .collect::<Vec<_>>();
        if matches.is_empty() {
            continue;
        }
        let name = glob_meta[matches[0]].plugin_name.clone();
        let entry = EntryPoint {
            path: files[idx].path.clone(),
            source: EntryPointSource::Plugin { name },
        };
        categorize_plugin_match(entries, entry, glob_meta, &matches);
    }
}

fn categorize_plugin_match(
    entries: &mut crate::discover::CategorizedEntryPoints,
    entry: EntryPoint,
    glob_meta: &[CompiledEntryRule],
    matches: &[usize],
) {
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

fn push_plugin_setup_files(
    entries: &mut crate::discover::CategorizedEntryPoints,
    root: &Path,
    plugin_result: &crate::plugins::AggregatedPluginResult,
) {
    for setup_file in plugin_result.setup_files() {
        let resolved = resolve_plugin_setup_file(root, &setup_file.path);
        if resolved.exists() {
            entries.push_support(EntryPoint {
                path: resolved,
                source: EntryPointSource::Plugin {
                    name: setup_file.plugin_name,
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
                        name: setup_file.plugin_name.clone(),
                    },
                });
                break;
            }
        }
    }
}

fn resolve_plugin_setup_file(root: &Path, setup_file: &Path) -> PathBuf {
    if is_absolute_path_any_platform(setup_file) {
        setup_file.to_path_buf()
    } else {
        root.join(setup_file)
    }
}

struct CompiledEntryRule {
    path: CompiledPathRule,
    plugin_name: String,
    role: EntryPointRole,
}

impl CompiledEntryRule {
    fn matches(&self, path: &str) -> bool {
        self.path.matches(path)
    }
}

fn compile_plugin_entry_rule(
    entry_pattern: &crate::plugins::PluginEntryPattern,
    plugin_result: &crate::plugins::AggregatedPluginResult,
) -> Option<(globset::Glob, CompiledEntryRule)> {
    let include = match globset::GlobBuilder::new(&entry_pattern.rule.pattern)
        .literal_separator(true)
        .build()
    {
        Ok(glob) => glob,
        Err(err) => {
            tracing::warn!(
                "invalid entry pattern '{}': {err}",
                entry_pattern.rule.pattern
            );
            return None;
        }
    };
    let role = plugin_result.entry_point_role(&entry_pattern.plugin_name);
    Some((
        include,
        CompiledEntryRule {
            path: CompiledPathRule::for_entry_rule(&entry_pattern.rule, "entry pattern")?,
            plugin_name: entry_pattern.plugin_name.clone(),
            role,
        },
    ))
}

#[derive(Debug, Clone)]
struct CompiledPathRule {
    include: globset::GlobMatcher,
    exclude_globs: Vec<globset::GlobMatcher>,
    exclude_regexes: Vec<Regex>,
    exclude_segment_regexes: Vec<Regex>,
}

impl CompiledPathRule {
    fn for_entry_rule(rule: &crate::plugins::PluginPathRule, rule_kind: &str) -> Option<Self> {
        let include = match globset::GlobBuilder::new(&rule.pattern)
            .literal_separator(true)
            .build()
        {
            Ok(glob) => glob.compile_matcher(),
            Err(err) => {
                tracing::warn!("invalid {rule_kind} '{}': {err}", rule.pattern);
                return None;
            }
        };
        Some(Self {
            include,
            exclude_globs: compile_excluded_globs(&rule.exclude_globs, rule_kind, &rule.pattern),
            exclude_regexes: compile_excluded_regexes(
                &rule.exclude_regexes,
                rule_kind,
                &rule.pattern,
            ),
            exclude_segment_regexes: compile_excluded_regexes(
                &rule.exclude_segment_regexes,
                rule_kind,
                &rule.pattern,
            ),
        })
    }

    fn matches(&self, path: &str) -> bool {
        self.include.is_match(path)
            && !self.exclude_globs.iter().any(|glob| glob.is_match(path))
            && !self
                .exclude_regexes
                .iter()
                .any(|regex| regex.is_match(path))
            && !path.split('/').any(|segment| {
                self.exclude_segment_regexes
                    .iter()
                    .any(|regex| regex.is_match(segment))
            })
    }
}

fn compile_excluded_globs(
    patterns: &[String],
    rule_kind: &str,
    rule_pattern: &str,
) -> Vec<globset::GlobMatcher> {
    patterns
        .iter()
        .filter_map(|pattern| {
            match globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
            {
                Ok(glob) => Some(glob.compile_matcher()),
                Err(err) => {
                    tracing::warn!(
                        "skipping invalid excluded glob '{}' for {} '{}': {err}",
                        pattern,
                        rule_kind,
                        rule_pattern
                    );
                    None
                }
            }
        })
        .collect()
}

fn compile_excluded_regexes(
    patterns: &[String],
    rule_kind: &str,
    rule_pattern: &str,
) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|pattern| match Regex::new(pattern) {
            Ok(regex) => Some(regex),
            Err(err) => {
                tracing::warn!(
                    "skipping invalid excluded regex '{}' for {} '{}': {err}",
                    pattern,
                    rule_kind,
                    rule_pattern
                );
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::{FallowConfig, OutputFormat};
    use fallow_types::discover::FileId;

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

    fn discovered(root: &Path, rel: &str, id: u32) -> DiscoveredFile {
        DiscoveredFile {
            id: FileId(id),
            path: root.join(rel),
            size_bytes: 1,
        }
    }

    #[test]
    fn root_entry_points_use_package_json_scripts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("scripts")).expect("scripts dir");
        std::fs::write(root.join("scripts/build.ts"), "export const build = true;")
            .expect("script file");
        std::fs::write(
            root.join("package.json"),
            r#"{"scripts":{"build":"tsx scripts/build.ts"}}"#,
        )
        .expect("package file");
        let config = config_for(root);
        let files = [discovered(root, "scripts/build.ts", 0)];

        let entries = discover_entry_points(&config, &files);

        assert_eq!(entries.len(), 1);
        assert!(entries[0].path.ends_with("scripts/build.ts"));
        assert!(matches!(
            entries[0].source,
            EntryPointSource::PackageJsonScript
        ));
    }

    #[test]
    fn root_entry_points_fall_back_to_default_index() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).expect("src dir");
        std::fs::write(root.join("src/index.ts"), "export const main = true;").expect("index file");
        let config = config_for(root);
        let files = [discovered(root, "src/index.ts", 0)];

        let entries = discover_entry_points(&config, &files);

        assert_eq!(entries.len(), 1);
        assert!(entries[0].path.ends_with("src/index.ts"));
        assert!(matches!(entries[0].source, EntryPointSource::DefaultIndex));
    }

    #[test]
    fn workspace_entry_points_use_workspace_package_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let ws_root = root.join("packages/app");
        std::fs::create_dir_all(ws_root.join("src")).expect("workspace src dir");
        std::fs::write(ws_root.join("src/main.ts"), "export const main = true;")
            .expect("workspace main");
        std::fs::write(ws_root.join("package.json"), r#"{"main":"src/main.ts"}"#)
            .expect("workspace package file");
        let config = config_for(root);
        let files = [discovered(root, "packages/app/src/main.ts", 0)];

        let entries = discover_workspace_entry_points(&ws_root, &config, &files);

        assert_eq!(entries.len(), 1);
        assert!(entries[0].path.ends_with("packages/app/src/main.ts"));
        assert!(matches!(
            entries[0].source,
            EntryPointSource::PackageJsonMain
        ));
    }
}
