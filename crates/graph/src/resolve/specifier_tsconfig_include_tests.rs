//! Tests for the scope of `include` patterns in a referenced tsconfig.

use std::fs;
use std::path::Path;

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

/// A referenced tsconfig in a directory whose name holds glob syntax, such
/// as a Next.js `[id]` route segment, applies to its files. The directory is
/// literal text, so the config stays in the chain and its `paths` apply.
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn referenced_tsconfig_in_glob_syntax_directory_applies_to_its_files() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    let app = root.join("app/[id]");
    fs::create_dir_all(app.join("src")).unwrap();
    fs::write(app.join("src/index.ts"), "").unwrap();
    fs::write(
        root.join("tsconfig.json"),
        r#"{"files":[],"references":[{"path":"./app/[id]/tsconfig.app.json"}]}"#,
    )
    .unwrap();
    fs::write(
        app.join("tsconfig.app.json"),
        r#"{"compilerOptions":{"paths":{"@lib/*":["./src/*"]}},"include":["./src/**/*"]}"#,
    )
    .unwrap();
    let from_file = app.join("src/index.ts");

    with_ctx_at_root(root, |ctx| {
        let chain = super::local_tsconfig_chain(ctx, &from_file);
        assert!(
            chain
                .iter()
                .any(|path| path.ends_with("[id]/tsconfig.app.json")),
            "the referenced config must stay in the chain, got {chain:?}"
        );
        assert!(super::matches_nearest_tsconfig_path_alias(
            ctx,
            &from_file,
            "@lib/util"
        ));
    });
}

/// Glob syntax in the tsconfig directory matches only that literal
/// directory, while a wildcard in the entry keeps its glob meaning.
#[test]
fn glob_values_match_treats_tsconfig_directory_as_literal() {
    let include = [serde_json::json!("src/**/*.ts")];
    for dir in ["[id]", "{a,b}", "*", "?"] {
        let base = Path::new("/repo").join(dir);
        assert!(
            super::glob_values_match(&base, &include, &base.join("src/index.ts")),
            "a file under `{dir}` must match"
        );
    }
    let base = Path::new("/repo/[id]");
    assert!(!super::glob_values_match(
        base,
        &include,
        Path::new("/repo/i/src/index.ts")
    ));
    let base = Path::new("/repo/{a,b}");
    assert!(!super::glob_values_match(
        base,
        &include,
        Path::new("/repo/a/src/index.ts")
    ));
    let base = Path::new("/repo/*");
    assert!(!super::glob_values_match(
        base,
        &include,
        Path::new("/repo/other/src/index.ts")
    ));
    assert!(!super::glob_values_match(
        base,
        &include,
        Path::new("/repo/*/src/index.js")
    ));
}

/// A `..` entry leaves the tsconfig directory before glob syntax starts.
#[test]
fn glob_values_match_resolves_parent_segments_before_the_glob() {
    let include = [serde_json::json!("../shared/**/*.ts")];
    let base = Path::new("/repo/[id]/app");
    assert!(super::glob_values_match(
        base,
        &include,
        Path::new("/repo/[id]/shared/util.ts")
    ));
    assert!(!super::glob_values_match(
        base,
        &include,
        Path::new("/repo/[id]/app/shared/util.ts")
    ));
}

/// A tsconfig in the file system root joins its entries without a second `/`.
#[test]
fn glob_values_match_handles_tsconfig_in_root_directory() {
    let include = [serde_json::json!("**/*")];
    assert!(super::glob_values_match(
        Path::new("/"),
        &include,
        Path::new("/a/b.ts")
    ));
}

/// tsc reads only `*` and `?` as wildcards, so brackets and braces in an
/// entry, such as a Next.js `app/[id]` route segment, are literal text.
#[test]
fn glob_values_match_treats_brackets_and_braces_in_entry_as_literal() {
    let include = [
        serde_json::json!("app/[id]/**/*"),
        serde_json::json!("lib/{a,b}/*.ts"),
    ];
    let base = Path::new("/repo");
    assert!(super::glob_values_match(
        base,
        &include,
        Path::new("/repo/app/[id]/page.ts")
    ));
    assert!(!super::glob_values_match(
        base,
        &include,
        Path::new("/repo/app/i/page.ts")
    ));
    assert!(super::glob_values_match(
        base,
        &include,
        Path::new("/repo/lib/{a,b}/x.ts")
    ));
    assert!(!super::glob_values_match(
        base,
        &include,
        Path::new("/repo/lib/a/x.ts")
    ));
}
