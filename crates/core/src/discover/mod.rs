mod entry_points;
mod infrastructure;
mod parse_scripts;
mod walk;

use std::path::{Component, Path};

use fallow_config::{PackageJson, ResolvedConfig};
use rustc_hash::FxHashSet;

pub use entry_points::{
    CategorizedEntryPoints, compile_glob_set, discover_dynamically_loaded_entry_points,
    discover_entry_points, discover_plugin_entry_point_sets, discover_plugin_entry_points,
    discover_workspace_entry_points,
};
pub(crate) use entry_points::{
    EntryPointDiscovery, discover_entry_points_with_warnings_from_pkg,
    discover_workspace_entry_points_with_warnings_from_pkg, resolve_entry_path,
    warn_skipped_entry_summary,
};
pub use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
pub(crate) use infrastructure::discover_infrastructure_entry_points;
pub use walk::{
    HiddenDirScope, PRODUCTION_EXCLUDE_PATTERNS, SOURCE_EXTENSIONS, discover_files,
    discover_files_and_config_candidates, discover_files_with_additional_hidden_dirs,
    is_allowed_hidden_dir,
};

/// Collect package-scoped hidden directory traversal rules for active plugins.
///
/// Source discovery runs before full plugin execution, so this consults
/// package-activation checks and static plugin metadata only. Callers that
/// also need script-derived scopes should use `collect_hidden_dir_scopes`
/// instead, which loads each workspace's `package.json` once and feeds both
/// passes; standalone CLI command paths can use
/// [`discover_files_with_plugin_scopes`] when they have neither already.
#[must_use]
pub fn collect_plugin_hidden_dir_scopes(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> Vec<HiddenDirScope> {
    let registry = crate::plugins::PluginRegistry::new(config.external_plugins.clone());
    let mut scopes = Vec::new();

    if let Some(pkg) = root_pkg {
        push_plugin_hidden_dir_scope(&mut scopes, &registry, pkg, &config.root);
    }

    for ws in workspaces {
        if let Ok(pkg) = PackageJson::load(&ws.root.join("package.json")) {
            push_plugin_hidden_dir_scope(&mut scopes, &registry, &pkg, &ws.root);
        }
    }

    scopes
}

/// Combined plugin-derived and script-derived hidden directory scopes.
///
/// Loads each workspace's `package.json` ONCE and feeds both the plugin
/// registry's `discovery_hidden_dirs` check and the
/// `package.json#scripts` extractor. Prefer this over calling
/// [`collect_plugin_hidden_dir_scopes`] and
/// [`collect_script_hidden_dir_scopes`] back-to-back: on monorepos with
/// many workspace packages, doing the workspace `package.json` read once
/// avoids quadratic I/O.
#[must_use]
pub(crate) fn collect_hidden_dir_scopes(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> Vec<HiddenDirScope> {
    let _span = tracing::info_span!("collect_hidden_dir_scopes").entered();
    let registry = crate::plugins::PluginRegistry::new(config.external_plugins.clone());
    let mut scopes = Vec::new();

    if let Some(pkg) = root_pkg {
        push_plugin_hidden_dir_scope(&mut scopes, &registry, pkg, &config.root);
        if let Some(scope) = build_script_scope(pkg, &config.root) {
            scopes.push(scope);
        }
    }

    for ws in workspaces {
        if let Ok(pkg) = PackageJson::load(&ws.root.join("package.json")) {
            push_plugin_hidden_dir_scope(&mut scopes, &registry, &pkg, &ws.root);
            if let Some(scope) = build_script_scope(&pkg, &ws.root) {
                scopes.push(scope);
            }
        }
    }

    scopes
}

fn push_plugin_hidden_dir_scope(
    scopes: &mut Vec<HiddenDirScope>,
    registry: &crate::plugins::PluginRegistry,
    pkg: &PackageJson,
    root: &Path,
) {
    let dirs = registry.discovery_hidden_dirs(pkg, root);
    if !dirs.is_empty() {
        scopes.push(HiddenDirScope::new(root.to_path_buf(), dirs));
    }
}

/// Discover files with plugin-aware hidden directory traversal.
///
/// Convenience wrapper for command paths (list, dupes, health, flags, coverage)
/// that don't already have workspaces / root `package.json` on hand. Internally
/// loads the root `package.json` and discovers workspaces so plugin-contributed
/// hidden directories (e.g. React Router's `.client` / `.server` folders) AND
/// hidden directories referenced from `package.json#scripts` (e.g.
/// `eslint -c .config/eslint.config.js`) are traversed consistently across
/// every command.
#[must_use]
pub fn discover_files_with_plugin_scopes(config: &ResolvedConfig) -> Vec<DiscoveredFile> {
    let root_pkg = PackageJson::load(&config.root.join("package.json")).ok();
    let workspaces = fallow_config::discover_workspaces(&config.root);
    let scopes = collect_hidden_dir_scopes(config, root_pkg.as_ref(), &workspaces);
    discover_files_with_additional_hidden_dirs(config, &scopes)
}

/// Hidden (dot-prefixed) directories that should be included in file discovery.
///
/// Most hidden directories (`.git`, `.cache`, etc.) should be skipped, but certain
/// convention directories contain source or config files that fallow needs to see:
/// - `.storybook` — Storybook configuration (the Storybook plugin depends on this)
/// - `.vitepress` — VitePress configuration and theme files
/// - `.well-known` — Standard web convention directory
/// - `.changeset` — Changesets configuration
/// - `.github` — GitHub workflows and CI scripts
const ALLOWED_HIDDEN_DIRS: &[&str] = &[
    ".storybook",
    ".vitepress",
    ".well-known",
    ".changeset",
    ".github",
];

/// Hidden directories that must NEVER be auto-scoped from a `package.json#scripts`
/// reference. These are build caches, VCS metadata, IDE state, or package-manager
/// state where walking would tank performance or pollute analysis. A script that
/// happens to read or write into one of these directories (e.g. `nx run ... && cp
/// dist/foo .nx/cache/`) must not pull the entire directory into source discovery.
const SCRIPT_SCOPE_DENYLIST: &[&str] = &[
    ".git",
    ".next",
    ".nuxt",
    ".output",
    ".svelte-kit",
    ".turbo",
    ".nx",
    ".cache",
    ".parcel-cache",
    ".vercel",
    ".netlify",
    ".yarn",
    ".pnpm-store",
    ".docusaurus",
    ".vscode",
    ".idea",
    ".fallow",
    ".husky",
];

/// Collect package-scoped hidden directory traversal rules from
/// `package.json#scripts` references.
///
/// Many tools accept custom config paths via `--config` / `-c` flags or positional
/// file arguments (e.g. `eslint -c .config/eslint.config.js`,
/// `vitest --config .config/vitest.config.ts`, `tsx ./.scripts/build.ts`). The file
/// walker's hidden-directory filter would otherwise skip `.config/` and friends,
/// leaving the referenced file out of the file registry. The file is detected as
/// an entry point but never parsed, so its imports are never credited.
///
/// Guardrails:
/// - Only the structured outputs of `crate::scripts::parse_script`
///   (`config_args`, `file_args`) are inspected. Arbitrary script tokens are not
///   scanned, so a logging path like `.nx/cache/result.json` in a script body
///   cannot pull `.nx/` into scope.
/// - Paths containing `..` are skipped. A workspace script referencing
///   `../../.config/...` should not generate a scope rooted at that workspace.
/// - `SCRIPT_SCOPE_DENYLIST` excludes known build-cache, VCS, IDE, and
///   package-manager state directories regardless of script content.
#[must_use]
pub fn collect_script_hidden_dir_scopes(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> Vec<HiddenDirScope> {
    let _span = tracing::info_span!("collect_script_hidden_dir_scopes").entered();
    let mut scopes = Vec::new();

    if let Some(pkg) = root_pkg
        && let Some(scope) = build_script_scope(pkg, &config.root)
    {
        scopes.push(scope);
    }
    for ws in workspaces {
        if let Ok(pkg) = PackageJson::load(&ws.root.join("package.json"))
            && let Some(scope) = build_script_scope(&pkg, &ws.root)
        {
            scopes.push(scope);
        }
    }
    scopes
}

fn build_script_scope(pkg: &PackageJson, root: &Path) -> Option<HiddenDirScope> {
    let scripts = pkg.scripts.as_ref()?;
    let mut seen = FxHashSet::default();
    let mut dirs: Vec<String> = Vec::new();

    for (script_name, script_value) in scripts {
        for cmd in crate::scripts::parse_script(script_value) {
            for path in cmd.config_args.iter().chain(cmd.file_args.iter()) {
                for hidden in extract_hidden_segments(path) {
                    if SCRIPT_SCOPE_DENYLIST.contains(&hidden.as_str()) {
                        continue;
                    }
                    if seen.insert(hidden.clone()) {
                        tracing::debug!(
                            dir = %hidden,
                            script = %script_name,
                            package_root = %root.display(),
                            "inferred hidden_dir_scope from package.json#scripts"
                        );
                        dirs.push(hidden);
                    }
                }
            }
        }
    }

    if dirs.is_empty() {
        None
    } else {
        Some(HiddenDirScope::new(root.to_path_buf(), dirs))
    }
}

/// Extract hidden (dot-prefixed) directory segments from a relative path.
///
/// Returns an empty vec when the path is absolute or contains any `..`
/// component, so scopes cannot escape a package root. Trailing file
/// components are not included (a path like `.config/eslint.config.js`
/// yields `[".config"]`, not `[".config", "eslint.config.js"]`).
///
/// A bare single-component path like `.env` is treated as a file (not a
/// directory) and yields empty. Real-world tools that accept a directory
/// as the value of `-c` are vanishingly rare; the common case is a file
/// path. Conflating the two would over-eagerly scope hidden filenames.
fn extract_hidden_segments(path: &str) -> Vec<String> {
    let p = Path::new(path);
    if p.is_absolute() {
        return Vec::new();
    }
    let components: Vec<Component> = p.components().collect();
    if components.iter().any(|c| matches!(c, Component::ParentDir)) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let upto = components.len().saturating_sub(1);
    for component in &components[..upto] {
        if let Component::Normal(name) = component {
            let s = name.to_string_lossy();
            if s.starts_with('.') && s.len() > 1 {
                out.push(s.into_owned());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_hidden_dirs_count() {
        assert_eq!(
            ALLOWED_HIDDEN_DIRS.len(),
            5,
            "update tests when adding new allowed hidden dirs"
        );
    }

    #[test]
    fn allowed_hidden_dirs_all_start_with_dot() {
        for dir in ALLOWED_HIDDEN_DIRS {
            assert!(
                dir.starts_with('.'),
                "allowed hidden dir '{dir}' must start with '.'"
            );
        }
    }

    #[test]
    fn allowed_hidden_dirs_no_duplicates() {
        let mut seen = rustc_hash::FxHashSet::default();
        for dir in ALLOWED_HIDDEN_DIRS {
            assert!(seen.insert(*dir), "duplicate allowed hidden dir: {dir}");
        }
    }

    #[test]
    fn allowed_hidden_dirs_no_trailing_slash() {
        for dir in ALLOWED_HIDDEN_DIRS {
            assert!(
                !dir.ends_with('/'),
                "allowed hidden dir '{dir}' should not have trailing slash"
            );
        }
    }

    #[test]
    fn file_id_re_exported() {
        let id = FileId(42);
        assert_eq!(id.0, 42);
    }

    #[test]
    fn source_extensions_re_exported() {
        assert!(SOURCE_EXTENSIONS.contains(&"ts"));
        assert!(SOURCE_EXTENSIONS.contains(&"tsx"));
    }

    #[test]
    fn compile_glob_set_re_exported() {
        let result = compile_glob_set(&["**/*.ts".to_string()]);
        assert!(result.is_some());
    }

    #[test]
    fn script_scope_denylist_all_start_with_dot() {
        for dir in SCRIPT_SCOPE_DENYLIST {
            assert!(
                dir.starts_with('.'),
                "denylisted dir '{dir}' must start with '.'"
            );
        }
    }

    #[test]
    fn script_scope_denylist_no_duplicates() {
        let mut seen = rustc_hash::FxHashSet::default();
        for dir in SCRIPT_SCOPE_DENYLIST {
            assert!(seen.insert(*dir), "duplicate denylisted dir: {dir}");
        }
    }

    #[test]
    fn script_scope_denylist_does_not_overlap_allowlist() {
        for dir in SCRIPT_SCOPE_DENYLIST {
            assert!(
                !ALLOWED_HIDDEN_DIRS.contains(dir),
                "denylisted dir '{dir}' must not also appear in ALLOWED_HIDDEN_DIRS"
            );
        }
    }

    #[test]
    fn extract_hidden_segments_single_segment() {
        assert_eq!(
            extract_hidden_segments(".config/eslint.config.js"),
            vec![".config".to_string()]
        );
    }

    #[test]
    fn extract_hidden_segments_with_leading_dot_slash() {
        assert_eq!(
            extract_hidden_segments("./.config/eslint.config.js"),
            vec![".config".to_string()]
        );
    }

    #[test]
    fn extract_hidden_segments_nested_hidden() {
        assert_eq!(
            extract_hidden_segments(".foo/.bar/x.js"),
            vec![".foo".to_string(), ".bar".to_string()]
        );
    }

    #[test]
    fn extract_hidden_segments_hidden_inside_normal_parent() {
        assert_eq!(
            extract_hidden_segments("sub/.config/eslint.config.js"),
            vec![".config".to_string()]
        );
    }

    #[test]
    fn extract_hidden_segments_no_hidden_returns_empty() {
        assert!(extract_hidden_segments("src/index.ts").is_empty());
    }

    #[test]
    fn extract_hidden_segments_skips_trailing_filename() {
        assert!(extract_hidden_segments(".env").is_empty());
        assert!(extract_hidden_segments("src/.eslintrc.js").is_empty());
    }

    #[test]
    fn extract_hidden_segments_skips_paths_with_parent_dir() {
        assert!(extract_hidden_segments("../.config/eslint.config.js").is_empty());
        assert!(extract_hidden_segments(".config/../other/x.js").is_empty());
        assert!(extract_hidden_segments("../../.config/eslint.config.js").is_empty());
    }

    #[test]
    fn extract_hidden_segments_skips_absolute_paths() {
        #[cfg(unix)]
        {
            assert!(extract_hidden_segments("/etc/.config/eslint.config.js").is_empty());
        }
        #[cfg(windows)]
        {
            assert!(extract_hidden_segments(r"C:\etc\.config\eslint.config.js").is_empty());
        }
    }

    #[test]
    fn extract_hidden_segments_ignores_bare_dot() {
        assert!(extract_hidden_segments(".").is_empty());
        assert!(extract_hidden_segments("./src/index.ts").is_empty());
    }

    #[expect(
        clippy::disallowed_types,
        reason = "PackageJson::scripts uses std HashMap for serde compatibility"
    )]
    fn make_pkg_with_scripts(entries: &[(&str, &str)]) -> PackageJson {
        let mut pkg = PackageJson::default();
        let mut scripts: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for (name, value) in entries {
            scripts.insert((*name).to_string(), (*value).to_string());
        }
        pkg.scripts = Some(scripts);
        pkg
    }

    fn make_config(root: std::path::PathBuf) -> ResolvedConfig {
        fallow_config::FallowConfig::default().resolve(
            root,
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        )
    }

    #[test]
    fn script_scope_extracts_dash_c_config_arg() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = make_config(dir.path().to_path_buf());
        let pkg = make_pkg_with_scripts(&[("lint", "eslint -c .config/eslint.config.js")]);
        let scopes = collect_script_hidden_dir_scopes(&config, Some(&pkg), &[]);

        assert_eq!(scopes.len(), 1, "one scope for the root package");
        let target_dir = dir.path().join(".config");
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(target_dir.join("eslint.config.js"), "export default {};").unwrap();
        let files = discover_files_with_additional_hidden_dirs(&config, &scopes);
        let names: Vec<String> = files
            .iter()
            .map(|f| {
                f.path
                    .strip_prefix(dir.path())
                    .unwrap_or(&f.path)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(
            names.contains(&".config/eslint.config.js".to_string()),
            "expected .config/eslint.config.js to be discovered; got {names:?}"
        );
    }

    #[test]
    fn script_scope_extracts_long_config_arg_with_equals() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = make_config(dir.path().to_path_buf());
        let pkg = make_pkg_with_scripts(&[("test", "vitest --config=.config/vitest.config.ts")]);
        let scopes = collect_script_hidden_dir_scopes(&config, Some(&pkg), &[]);
        assert_eq!(scopes.len(), 1);
    }

    #[test]
    fn script_scope_extracts_positional_file_arg() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = make_config(dir.path().to_path_buf());
        let pkg = make_pkg_with_scripts(&[("build", "tsx ./.scripts/build.ts")]);
        let scopes = collect_script_hidden_dir_scopes(&config, Some(&pkg), &[]);
        assert_eq!(scopes.len(), 1);
    }

    #[test]
    fn script_scope_denies_known_bad_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = make_config(dir.path().to_path_buf());
        let pkg = make_pkg_with_scripts(&[
            ("cache", "tsx .nx/scripts/cache.ts"),
            ("vscode", "node .vscode/build.js"),
            ("yarn-state", "node .yarn/releases/yarn-4.0.0.cjs"),
        ]);
        let scopes = collect_script_hidden_dir_scopes(&config, Some(&pkg), &[]);
        assert!(
            scopes.is_empty(),
            "denylisted dirs must not produce scopes; got {scopes:?}"
        );
    }

    #[test]
    fn script_scope_mixes_denied_and_allowed_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = make_config(dir.path().to_path_buf());
        let pkg = make_pkg_with_scripts(&[(
            "lint",
            "nx run-many --target=lint && eslint -c .config/eslint.config.js",
        )]);
        let scopes = collect_script_hidden_dir_scopes(&config, Some(&pkg), &[]);
        assert_eq!(scopes.len(), 1, "one scope for the .config reference");

        std::fs::create_dir_all(dir.path().join(".config")).unwrap();
        std::fs::write(
            dir.path().join(".config/eslint.config.js"),
            "export default {};",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".nx/cache")).unwrap();
        std::fs::write(dir.path().join(".nx/cache/build.js"), "// cache").unwrap();

        let files = discover_files_with_additional_hidden_dirs(&config, &scopes);
        let names: Vec<String> = files
            .iter()
            .map(|f| {
                f.path
                    .strip_prefix(dir.path())
                    .unwrap_or(&f.path)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(names.contains(&".config/eslint.config.js".to_string()));
        assert!(
            !names.contains(&".nx/cache/build.js".to_string()),
            "denylisted .nx must stay hidden"
        );
    }

    #[test]
    fn script_scope_skips_parent_dir_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = make_config(dir.path().to_path_buf());
        let pkg = make_pkg_with_scripts(&[("lint", "eslint -c ../../.config/eslint.config.js")]);
        let scopes = collect_script_hidden_dir_scopes(&config, Some(&pkg), &[]);
        assert!(
            scopes.is_empty(),
            "paths with .. must not generate scopes; got {scopes:?}"
        );
    }

    #[test]
    fn script_scope_no_scripts_returns_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = make_config(dir.path().to_path_buf());
        let pkg = PackageJson::default();
        let scopes = collect_script_hidden_dir_scopes(&config, Some(&pkg), &[]);
        assert!(scopes.is_empty());
    }

    #[test]
    fn script_scope_no_hidden_paths_returns_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = make_config(dir.path().to_path_buf());
        let pkg = make_pkg_with_scripts(&[
            ("build", "tsc -p tsconfig.json"),
            ("lint", "eslint -c eslint.config.js"),
        ]);
        let scopes = collect_script_hidden_dir_scopes(&config, Some(&pkg), &[]);
        assert!(scopes.is_empty());
    }

    #[test]
    fn script_scope_dedupes_within_package() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = make_config(dir.path().to_path_buf());
        let pkg = make_pkg_with_scripts(&[
            ("lint", "eslint -c .config/eslint.config.js"),
            ("test", "vitest --config .config/vitest.config.ts"),
        ]);
        let scopes = collect_script_hidden_dir_scopes(&config, Some(&pkg), &[]);
        assert_eq!(scopes.len(), 1);
    }

    #[test]
    fn script_scope_workspace_packages_have_own_scope_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = make_config(dir.path().to_path_buf());
        let ws_root = dir.path().join("packages/app");
        std::fs::create_dir_all(&ws_root).unwrap();
        let ws_pkg_path = ws_root.join("package.json");
        std::fs::write(
            &ws_pkg_path,
            r#"{"name":"app","scripts":{"lint":"eslint -c .config/eslint.config.js"}}"#,
        )
        .unwrap();
        let ws = fallow_config::WorkspaceInfo {
            root: ws_root.clone(),
            name: "app".to_string(),
            is_internal_dependency: false,
        };
        let scopes = collect_script_hidden_dir_scopes(&config, None, &[ws]);
        assert_eq!(scopes.len(), 1);

        std::fs::create_dir_all(ws_root.join(".config")).unwrap();
        std::fs::write(
            ws_root.join(".config/eslint.config.js"),
            "export default {};",
        )
        .unwrap();
        let other_root = dir.path().join("packages/other");
        std::fs::create_dir_all(other_root.join(".config")).unwrap();
        std::fs::write(
            other_root.join(".config/eslint.config.js"),
            "export default {};",
        )
        .unwrap();

        let files = discover_files_with_additional_hidden_dirs(&config, &scopes);
        let names: Vec<String> = files
            .iter()
            .map(|f| {
                f.path
                    .strip_prefix(dir.path())
                    .unwrap_or(&f.path)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(names.contains(&"packages/app/.config/eslint.config.js".to_string()));
        assert!(
            !names.contains(&"packages/other/.config/eslint.config.js".to_string()),
            "unscoped workspace must not get .config traversed"
        );
    }
}
