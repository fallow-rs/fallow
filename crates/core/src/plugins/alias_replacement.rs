//! Alias replacement files that the resolver never reaches.
//!
//! Fallow applies a Vite, Vitest or webpack alias only as a resolver fallback,
//! so the plugins of these tools share this check.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use super::PluginResult;

/// Source-file extensions an alias replacement may name. A mock alias always
/// points at a JS/TS file; directory targets (`@` -> `src`) have no extension
/// and are not seeded as entry points.
const ALIAS_SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"];

/// True when `spec` is a bare npm package specifier (not a relative path, URL,
/// `data:`, or `@/` / `~/` / `#` style path alias key).
pub(super) fn is_bare_package_specifier(spec: &str) -> bool {
    crate::resolve::is_bare_specifier(spec)
        && crate::resolve::is_valid_package_name(spec)
        && !crate::resolve::is_path_alias(spec)
}

/// True when a normalized alias replacement names a local source file (by
/// extension), as opposed to a directory.
pub(super) fn alias_target_is_source_file(normalized: &str) -> bool {
    Path::new(normalized)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ALIAS_SOURCE_EXTENSIONS.contains(&ext))
}

/// True when an alias key is a relative or absolute file specifier, such as
/// `./clock.js`. An import with such a specifier resolves to a file on disk, so
/// the path-alias fallback of the resolver never routes it to the replacement.
fn is_path_specifier(find: &str) -> bool {
    find.starts_with("./") || find.starts_with("../") || find.starts_with('/')
}

/// Credit every export of an alias replacement file when the resolver never
/// routes an import to it. The import of the original target is then the only
/// consumer, so no import names the exports of the replacement file.
///
/// The resolver applies a plugin alias only as a fallback, which misses two
/// cases:
/// - A relative or absolute key, such as `./clock.js`, resolves to the
///   original file (issue #3963).
/// - A bare key for an installed package, such as `pkg` with `node_modules/pkg`
///   present, resolves to the real package.
/// - A key that tsconfig `paths` or `baseUrl` maps to another existing file,
///   such as `src/clock` with the `paths` pattern `src/*`, resolves to that
///   file (issue #4035).
///
/// A bare key for a missing package and a path-alias key (`~/lib`, `@lib`)
/// that tsconfig does not map reach the replacement file through the
/// resolver, so their exports keep precise tracking. A scope-only key such as
/// `@lib` counts as a bare key, but no installed package has that name.
///
/// Returns whether the replacement got the credit, so a caller with no other
/// seed for the replacement file can also make it an entry point.
pub(super) fn credit_unrouted_replacement(
    result: &mut PluginResult,
    find: &str,
    normalized: &str,
    config_path: &Path,
    root: &Path,
) -> bool {
    if !alias_target_is_source_file(normalized) {
        return false;
    }
    let unrouted = is_path_specifier(find)
        || (is_bare_package_specifier(find) && is_installed_package(find, config_path))
        || tsconfig_maps_elsewhere(find, &root.join(normalized), config_path);
    if unrouted {
        result.push_entry_export_credit(globset::escape(normalized), ["*"]);
    }
    unrouted
}

/// True when the package of `specifier` is installed in a `node_modules`
/// directory at or above the config directory. A package that only a nested
/// workspace package installs is not found, so its replacement keeps the
/// current behavior.
fn is_installed_package(specifier: &str, config_path: &Path) -> bool {
    let Some(config_dir) = config_path.parent() else {
        return false;
    };
    let pkg_name = crate::resolve::extract_package_name(specifier);
    super::eslint::find_package_dir(config_dir, &pkg_name).is_some()
}

/// Maximum number of directories to walk up from a start directory to find a
/// `tsconfig.json`.
const MAX_TSCONFIG_WALK_DEPTH: usize = 12;

/// The tsconfig options that decide where a non-relative specifier resolves.
#[derive(Default)]
struct TsconfigModuleOptions {
    /// The `compilerOptions.paths` object and the folders that its targets
    /// resolve against, in lookup order.
    paths: Option<(serde_json::Map<String, serde_json::Value>, Vec<PathBuf>)>,
    /// The `compilerOptions.baseUrl` value, resolved against its tsconfig.
    base_url: Option<PathBuf>,
}

/// True when a tsconfig maps `find` to an existing file that is not
/// `replacement`. The resolver applies tsconfig `paths` and `baseUrl` before
/// the alias fallback, so the import then never reaches the replacement file.
///
/// The importers of `find` are not known here. The check reads the nearest
/// `tsconfig.json` at or above the replacement directory and at or above the
/// config directory, and the tsconfig files that each one lists in
/// `references`. A root config of a monorepo often has no tsconfig of its own,
/// and a solution-style tsconfig keeps `paths` in a referenced file.
fn tsconfig_maps_elsewhere(find: &str, replacement: &Path, config_path: &Path) -> bool {
    let root = crate::discover::project_read::config_project_root(
        config_path.parent().unwrap_or(config_path),
    );
    let mut seen: FxHashSet<PathBuf> = FxHashSet::default();
    [replacement.parent(), config_path.parent()]
        .into_iter()
        .flatten()
        .filter_map(nearest_tsconfig)
        .flat_map(|tsconfig| {
            let references = referenced_tsconfigs(&root, &tsconfig);
            std::iter::once(tsconfig).chain(references)
        })
        .filter(|tsconfig| seen.insert(tsconfig.clone()))
        .any(|tsconfig| {
            let options = read_tsconfig_module_options(&tsconfig);
            tsconfig_target(find, &options).is_some_and(|target| !same_file(&target, replacement))
        })
}

/// The nearest `tsconfig.json` at or above `dir`.
fn nearest_tsconfig(dir: &Path) -> Option<PathBuf> {
    fallow_types::audit_snapshot::lookup_ancestors(dir)
        .take(MAX_TSCONFIG_WALK_DEPTH)
        .map(|ancestor| ancestor.join("tsconfig.json"))
        .find(|candidate| candidate.is_file())
}

/// The tsconfig files that the `references` array of `tsconfig` names. A
/// reference to a directory names the `tsconfig.json` in that directory.
fn referenced_tsconfigs(root: &Path, tsconfig: &Path) -> Vec<PathBuf> {
    let Some(json) = read_jsonc(root, tsconfig) else {
        return Vec::new();
    };
    let dir = tsconfig.parent().unwrap_or(tsconfig);
    json.get("references")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|reference| reference.get("path")?.as_str())
        .map(|path| {
            let path = dir.join(path);
            if path.is_dir() {
                path.join("tsconfig.json")
            } else {
                path
            }
        })
        .filter(|path| path.is_file())
        .collect()
}

/// Read the merged `paths` and `baseUrl` of a tsconfig and its `extends`
/// chain.
fn read_tsconfig_module_options(tsconfig: &Path) -> TsconfigModuleOptions {
    let Some(merged) = fallow_config::MergedTsconfig::read(tsconfig) else {
        return TsconfigModuleOptions::default();
    };
    TsconfigModuleOptions {
        paths: merged.paths().map(|paths| {
            let bases = paths.base_dirs().map(Path::to_path_buf).collect();
            (paths.patterns.clone(), bases)
        }),
        base_url: merged.path_option("baseUrl"),
    }
}

/// Read a JSONC project file, such as a tsconfig, in the read scope of
/// `root`.
fn read_jsonc(root: &Path, path: &Path) -> Option<serde_json::Value> {
    let content = crate::discover::project_read::read_project_file(root, path)?;
    fallow_config::jsonc::parse_to_value(content.trim_start_matches('\u{FEFF}')).ok()
}

/// The existing file that tsconfig `paths` or `baseUrl` maps `specifier` to.
/// When no `paths` target exists, tsc tries `baseUrl` next.
fn tsconfig_target(specifier: &str, options: &TsconfigModuleOptions) -> Option<PathBuf> {
    let from_paths = options.paths.as_ref().and_then(|(paths, bases)| {
        let (targets, capture) = best_paths_match(paths, specifier)?;
        // Every target against `baseUrl` first, as TypeScript 5 resolves
        // them, then every target against the `paths` config folder.
        bases.iter().find_map(|base| {
            targets
                .iter()
                .filter_map(serde_json::Value::as_str)
                .find_map(|target| probe_source_file(&base.join(target.replacen('*', capture, 1))))
        })
    });
    from_paths.or_else(|| {
        options
            .base_url
            .as_ref()
            .and_then(|base_url| probe_source_file(&base_url.join(specifier)))
    })
}

/// The targets and the wildcard capture of the `paths` pattern that matches
/// `specifier`. An exact pattern wins, then the wildcard pattern with the
/// longest prefix, as in tsc.
fn best_paths_match<'a>(
    paths: &'a serde_json::Map<String, serde_json::Value>,
    specifier: &'a str,
) -> Option<(&'a [serde_json::Value], &'a str)> {
    if let Some(targets) = paths.get(specifier).and_then(serde_json::Value::as_array) {
        return Some((targets, ""));
    }
    paths
        .iter()
        .filter_map(|(pattern, targets)| {
            let (prefix, suffix) = pattern.split_once('*')?;
            let capture = specifier.strip_prefix(prefix)?.strip_suffix(suffix)?;
            Some((prefix.len(), targets.as_array()?.as_slice(), capture))
        })
        .max_by_key(|(prefix_len, _, _)| *prefix_len)
        .map(|(_, targets, capture)| (targets, capture))
}

/// The first existing source file for a module path: the path itself, the
/// path with a source extension, or an index file in the path directory.
fn probe_source_file(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let with_extension = ALIAS_SOURCE_EXTENSIONS.iter().map(|ext| {
        let mut file = path.as_os_str().to_owned();
        file.push(".");
        file.push(ext);
        PathBuf::from(file)
    });
    let index = ALIAS_SOURCE_EXTENSIONS
        .iter()
        .map(|ext| path.join(format!("index.{ext}")));
    with_extension.chain(index).find(|file| file.is_file())
}

/// True when two paths name the same file on disk.
fn same_file(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tsconfig that links out of the project is not read for its
    /// `references`.
    #[test]
    #[cfg(unix)]
    fn a_tsconfig_that_links_out_of_the_project_is_not_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&outside).expect("create outside");
        std::fs::write(
            outside.join("tsconfig.json"),
            r#"{"references":[{"path":"./app"}]}"#,
        )
        .expect("write");
        std::os::unix::fs::symlink(outside.join("tsconfig.json"), root.join("tsconfig.json"))
            .expect("link");
        std::fs::create_dir_all(root.join("app")).expect("create app");
        std::fs::write(root.join("app/tsconfig.json"), "{}").expect("write");
        assert!(referenced_tsconfigs(&root, &root.join("tsconfig.json")).is_empty());
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(path, contents).expect("write file");
    }

    fn credits(find: &str, root: &Path) -> Vec<crate::plugins::UsedExportRule> {
        let mut result = PluginResult::default();
        credit_unrouted_replacement(
            &mut result,
            find,
            "test/stubs/clock.ts",
            &root.join("vitest.config.ts"),
            root,
        );
        result.entry_export_credits
    }

    fn project(tsconfig: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        write(&dir.path().join("tsconfig.json"), tsconfig);
        write(&dir.path().join("src/clock.ts"), "export const now = 1;\n");
        write(
            &dir.path().join("test/stubs/clock.ts"),
            "export const now = 0;\n",
        );
        dir
    }

    #[test]
    fn tsconfig_paths_key_credits_every_replacement_export() {
        let dir = project(r#"{ "compilerOptions": { "paths": { "src/*": ["./src/*"] } } }"#);
        assert_eq!(credits("src/clock", dir.path()).len(), 1);
    }

    #[test]
    fn tsconfig_paths_from_relative_extends_credit_replacement_exports() {
        let dir = project(r#"{ "extends": "./tsconfig.base" }"#);
        write(
            &dir.path().join("tsconfig.base.json"),
            r#"{
                // JSONC comment
                "compilerOptions": { "paths": { "~/*": ["./src/*"] } },
            }"#,
        );
        assert_eq!(credits("~/clock", dir.path()).len(), 1);
    }

    #[test]
    fn tsconfig_paths_from_array_extends_credit_replacement_exports() {
        let dir = project(r#"{ "extends": ["./tsconfig.other.json", "./tsconfig.base.json"] }"#);
        write(
            &dir.path().join("tsconfig.other.json"),
            r#"{ "compilerOptions": {} }"#,
        );
        write(
            &dir.path().join("tsconfig.base.json"),
            r#"{ "compilerOptions": { "paths": { "src/*": ["./src/*"] } } }"#,
        );
        assert_eq!(credits("src/clock", dir.path()).len(), 1);
    }

    #[test]
    fn later_array_extends_entry_overrides_earlier_entry() {
        let dir = project(r#"{ "extends": ["./tsconfig.first.json", "./tsconfig.last.json"] }"#);
        write(
            &dir.path().join("tsconfig.first.json"),
            r#"{ "compilerOptions": { "paths": { "~/*": ["./src/*"] } } }"#,
        );
        write(
            &dir.path().join("tsconfig.last.json"),
            r#"{ "compilerOptions": { "paths": { "~/*": ["./test/stubs/*"] } } }"#,
        );
        assert!(credits("~/clock", dir.path()).is_empty());
    }

    #[test]
    fn every_extends_shape_gives_paths_and_base_url() {
        for shape in fallow_config::TsconfigExtendsShape::ALL {
            let dir = tempfile::tempdir().expect("temp dir");
            shape.write(dir.path());
            write(&dir.path().join("src/clock.ts"), "export const now = 1;\n");
            write(
                &dir.path().join("test/stubs/clock.ts"),
                "export const now = 0;\n",
            );
            assert_eq!(
                credits("@app/clock", dir.path()).len(),
                1,
                "{}: paths",
                shape.name()
            );
            assert_eq!(
                credits("src/clock", dir.path()).len(),
                1,
                "{}: baseUrl",
                shape.name()
            );
        }
    }

    #[test]
    fn every_paths_target_tries_base_url_before_the_paths_config_folder() {
        // TypeScript 5 tries all targets against `baseUrl` first, so the
        // later target `b.ts` at the root wins over `app/a.ts`.
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        write(
            &root.join("base.json"),
            r#"{"compilerOptions":{"baseUrl":"."}}"#,
        );
        write(
            &root.join("app/tsconfig.json"),
            r#"{"extends":"../base.json","compilerOptions":{"paths":{"@x":["./a.ts","./b.ts"]}}}"#,
        );
        write(&root.join("b.ts"), "export const b = 1;\n");
        write(&root.join("app/a.ts"), "export const a = 1;\n");
        let options = read_tsconfig_module_options(&root.join("app/tsconfig.json"));
        assert_eq!(
            tsconfig_target("@x", &options).map(|target| same_file(&target, &root.join("b.ts"))),
            Some(true)
        );
    }

    #[test]
    fn tsconfig_base_url_key_credits_every_replacement_export() {
        let dir = project(r#"{ "compilerOptions": { "baseUrl": "." } }"#);
        assert_eq!(credits("src/clock", dir.path()).len(), 1);
    }

    #[test]
    fn tsconfig_mapping_to_the_replacement_keeps_precise_tracking() {
        let dir = project(
            r#"{ "compilerOptions": { "paths": { "clock": ["./test/stubs/clock.ts"] } } }"#,
        );
        assert!(credits("clock", dir.path()).is_empty());
    }

    #[test]
    fn tsconfig_mapping_to_a_missing_file_keeps_precise_tracking() {
        let dir = project(r#"{ "compilerOptions": { "paths": { "src/*": ["./lib/*"] } } }"#);
        assert!(credits("src/clock", dir.path()).is_empty());
    }

    #[test]
    fn unmapped_key_keeps_precise_tracking() {
        let dir = project(r#"{ "compilerOptions": { "paths": { "@/*": ["./src/*"] } } }"#);
        assert!(credits("src/clock", dir.path()).is_empty());
    }

    #[test]
    fn referenced_tsconfig_near_the_replacement_credits_replacement_exports() {
        let dir = tempfile::tempdir().expect("temp dir");
        let app = dir.path().join("app");
        write(
            &app.join("tsconfig.json"),
            r#"{ "files": [], "references": [{ "path": "./tsconfig.app.json" }] }"#,
        );
        write(
            &app.join("tsconfig.app.json"),
            r#"{ "compilerOptions": { "paths": { "@/*": ["./src/*"] } } }"#,
        );
        write(&app.join("src/i18n/i18n.ts"), "export const t = 1;\n");
        write(&app.join("src/test/stubs/i18n.ts"), "export const t = 0;\n");
        let mut result = PluginResult::default();
        credit_unrouted_replacement(
            &mut result,
            "@/i18n/i18n",
            "app/src/test/stubs/i18n.ts",
            &dir.path().join("vitest.config.ts"),
            dir.path(),
        );
        assert_eq!(result.entry_export_credits.len(), 1);
    }

    #[test]
    fn longest_prefix_paths_pattern_wins() {
        let dir = project(
            r#"{ "compilerOptions": { "paths": { "*": ["./src/*"], "src/*": ["./test/stubs/*"] } } }"#,
        );
        assert!(credits("src/clock", dir.path()).is_empty());
    }
}
