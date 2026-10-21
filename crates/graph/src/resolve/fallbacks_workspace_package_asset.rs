//! Workspace package subpaths that name a file on disk that discovery does
//! not collect, such as `<pkg>/package.json`.

use std::path::Path;

use super::{PackageManifestInfo, ResolveResult, package_map_target_path};

/// Extensions of files that discovery analyzes (`SOURCE_EXTENSIONS` in
/// `fallow_core::discover`). A workspace package target with one of these
/// extensions that is not in the file set was left out on purpose (for
/// example by `ignorePatterns`), so it is not an asset.
const ANALYZABLE_EXTS: &[&str] = &[
    "ts", "tsx", "mts", "cts", "gts", "js", "jsx", "mjs", "cjs", "gjs", "vue", "svelte", "astro",
    "mdx", "css", "scss", "sass", "less", "html", "graphql", "gql",
];

/// Credit a workspace package for a subpath that names a non-source file on
/// disk, such as `<pkg>/package.json` or a `.wasm` file.
///
/// Discovery does not collect these files, so they have no file id. An
/// installed workspace package gives `NpmPackage` for the same import, so
/// this result keeps the dependency credit the same with and without
/// `node_modules`.
pub(super) fn workspace_package_asset(path: &Path, package_name: &str) -> Option<ResolveResult> {
    let is_source = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ANALYZABLE_EXTS.contains(&ext));
    (!is_source && path.is_file()).then(|| ResolveResult::NpmPackage(package_name.to_string()))
}

/// Credit a workspace package for the first `exports` target that names an
/// asset on disk.
pub(super) fn exports_target_asset(
    manifest: &PackageManifestInfo,
    targets: &[String],
    package_name: &str,
) -> Option<ResolveResult> {
    targets
        .iter()
        .filter_map(|target| package_map_target_path(manifest, target))
        .find_map(|path| workspace_package_asset(&path, package_name))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::tests::with_package_map_ctx;
    use super::super::try_workspace_package_fallback;
    use super::*;

    fn package_json_with_exports(exports: Option<serde_json::Value>) -> fallow_config::PackageJson {
        fallow_config::PackageJson {
            name: Some("egg".to_string()),
            main: Some("./dist/index.js".to_string()),
            exports,
            ..Default::default()
        }
    }

    fn workspace_package_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("package.json"), "{\"name\":\"egg\"}")
            .expect("write package.json");
        std::fs::create_dir_all(dir.path().join("src")).expect("create src");
        std::fs::write(dir.path().join("src/index.ts"), "export {};").expect("write source");
        std::fs::write(dir.path().join("src/x.ts"), "export {};").expect("write source");
        dir
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn workspace_package_json_subpath_with_exports_resolves_to_the_package() {
        let dir = workspace_package_dir();
        let exports = serde_json::json!({
            ".": "./src/index.ts",
            "./package.json": "./package.json"
        });
        with_package_map_ctx(
            dir.path().to_path_buf(),
            Some("egg"),
            package_json_with_exports(Some(exports)),
            &[],
            |ctx, _, _| {
                let result = try_workspace_package_fallback(ctx, "egg/package.json", false);
                assert!(matches!(result, Some(ResolveResult::NpmPackage(name)) if name == "egg"));
            },
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn workspace_package_json_subpath_without_exports_resolves_to_the_package() {
        let dir = workspace_package_dir();
        with_package_map_ctx(
            dir.path().to_path_buf(),
            Some("egg"),
            package_json_with_exports(None),
            &[],
            |ctx, _, _| {
                let result = try_workspace_package_fallback(ctx, "egg/package.json", false);
                assert!(matches!(result, Some(ResolveResult::NpmPackage(name)) if name == "egg"));
            },
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn workspace_package_json_subpath_not_in_exports_stays_blocked() {
        let dir = workspace_package_dir();
        let exports = serde_json::json!({ ".": "./src/index.ts" });
        with_package_map_ctx(
            dir.path().to_path_buf(),
            Some("egg"),
            package_json_with_exports(Some(exports)),
            &[],
            |ctx, _, _| {
                let result = try_workspace_package_fallback(ctx, "egg/package.json", false);
                assert!(result.is_none());
            },
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn workspace_source_target_missing_from_the_file_set_is_not_an_asset() {
        let dir = workspace_package_dir();
        let exports = serde_json::json!({ "./x": "./src/x.ts" });
        with_package_map_ctx(
            dir.path().to_path_buf(),
            Some("egg"),
            package_json_with_exports(Some(exports)),
            &[],
            |ctx, _, _| {
                let with_exports = try_workspace_package_fallback(ctx, "egg/x", false);
                assert!(!matches!(with_exports, Some(ResolveResult::NpmPackage(_))));
            },
        );
        with_package_map_ctx(
            dir.path().to_path_buf(),
            Some("egg"),
            package_json_with_exports(None),
            &[],
            |ctx, _, _| {
                let without_exports = try_workspace_package_fallback(ctx, "egg/src/x.ts", false);
                assert!(!matches!(
                    without_exports,
                    Some(ResolveResult::NpmPackage(_))
                ));
            },
        );
    }

    // --- try_workspace_package_fallback: bare-specifier guard (lines 967-968) ---

    #[test]
    fn try_workspace_package_fallback_rejects_relative_specifier() {
        // Relative specifiers (starting with "./" or "../") are not bare specifiers
        // and must return None without touching any manifest.
        let root = PathBuf::from("/project");
        let pj = fallow_config::PackageJson::default();
        with_package_map_ctx(root, None, pj, &[], |ctx, _manifest, _r| {
            let result = try_workspace_package_fallback(ctx, "./local/module", false);
            assert!(
                result.is_none(),
                "relative specifier should return None from workspace fallback"
            );
        });
    }

    #[test]
    fn try_workspace_package_fallback_rejects_absolute_path() {
        // Absolute paths are not bare specifiers either.
        let root = PathBuf::from("/project");
        let pj = fallow_config::PackageJson::default();
        with_package_map_ctx(root, None, pj, &[], |ctx, _manifest, _r| {
            let result = try_workspace_package_fallback(ctx, "/absolute/path", false);
            assert!(
                result.is_none(),
                "absolute path should return None from workspace fallback"
            );
        });
    }
}
