//! Tests for the scope of `include` patterns in a referenced tsconfig.

use std::fs;

use tempfile::tempdir;

use super::tests::with_ctx_at_root;
use crate::resolve::ResolveResult;

/// Issue #3946: a referenced tsconfig whose `include` starts with `./`
/// applies to its files, so its `paths` alias for an asset outside the
/// analyzed file set keeps the file instead of a bare package name.
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn referenced_tsconfig_paths_alias_resolves_existing_asset_file() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("app/javascript")).unwrap();
    fs::create_dir_all(root.join("app/assets/images")).unwrap();
    fs::write(root.join("app/javascript/index.ts"), "").unwrap();
    fs::write(root.join("app/assets/images/logo.png"), "").unwrap();
    fs::write(
        root.join("tsconfig.json"),
        r#"{"files":[],"references":[{"path":"./tsconfig.app.json"}]}"#,
    )
    .unwrap();
    fs::write(
        root.join("tsconfig.app.json"),
        r#"{"compilerOptions":{"paths":{"@circle-assets/*":["./app/assets/*"]}},"include":["./app/javascript/**/*"]}"#,
    )
    .unwrap();
    let from_file = root.join("app/javascript/index.ts");

    with_ctx_at_root(root, |ctx| {
        let result =
            super::resolve_specifier(ctx, &from_file, "@circle-assets/images/logo.png", false);
        let ResolveResult::ExternalFile(path) = result else {
            panic!("aliased asset must resolve to its file, got {result:?}");
        };
        assert_eq!(
            dunce::canonicalize(path).unwrap(),
            dunce::canonicalize(root.join("app/assets/images/logo.png")).unwrap()
        );

        let missing =
            super::resolve_specifier(ctx, &from_file, "@circle-assets/images/gone.png", false);
        assert!(
            !matches!(missing, ResolveResult::ExternalFile(_)),
            "a missing aliased asset must not resolve, got {missing:?}"
        );
    });
}
