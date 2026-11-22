//! The tsconfig `paths` fallback picks one pattern and falls back to
//! `baseUrl`, as TypeScript does.

use std::fs;

use fallow_types::discover::FileId;
use tempfile::tempdir;

use super::tests::with_ctx_at_root_and_files;
use crate::resolve::ResolveResult;
use crate::resolve::package_credit::nearest_tsconfig_alias_targets;

use super::{path_alias_capture, path_alias_specificity, select_path_alias};

/// A `paths` map that has no pattern for the specifier does not stop the
/// `baseUrl` lookup.
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn base_url_resolves_after_paths_has_no_matching_pattern() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("src/lib")).unwrap();
    fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":"src","paths":{"@/*":["src/*"]}}}"#,
    )
    .unwrap();
    fs::write(root.join("src/index.ts"), "").unwrap();
    fs::write(root.join("src/lib/util.ts"), "export {};").unwrap();
    let from_file = root.join("src/index.ts");
    let util = root.join("src/lib/util.ts");
    let files = [(util.as_path(), FileId(1))];

    with_ctx_at_root_and_files(root, &files, |ctx| {
        let result = super::try_nearest_tsconfig_path_alias(ctx, &from_file, "lib/util", false);
        assert!(
            matches!(result, Some(ResolveResult::InternalModule(FileId(1)))),
            "got {result:?}"
        );
    });
}

/// TypeScript uses only the matching pattern with the longest prefix. A
/// pattern with a shorter prefix and a longer suffix does not win, and its
/// targets are not tried.
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn paths_use_only_the_pattern_with_the_longest_prefix() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join("b/pp")).unwrap();
    fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"paths":{"@a*.styles":["b/*.styles"],"@app/*":["a/*"]}}}"#,
    )
    .unwrap();
    fs::write(root.join("index.ts"), "").unwrap();
    fs::write(root.join("a/x.styles.ts"), "export {};").unwrap();
    fs::write(root.join("b/pp/x.styles.ts"), "export {};").unwrap();
    let from_file = root.join("index.ts");
    let a = root.join("a/x.styles.ts");
    let b = root.join("b/pp/x.styles.ts");
    let files = [(a.as_path(), FileId(1)), (b.as_path(), FileId(2))];

    with_ctx_at_root_and_files(root, &files, |ctx| {
        let result =
            super::try_nearest_tsconfig_path_alias(ctx, &from_file, "@app/x.styles", false);
        assert!(
            matches!(result, Some(ResolveResult::InternalModule(FileId(1)))),
            "got {result:?}"
        );
        let missing =
            super::try_nearest_tsconfig_path_alias(ctx, &from_file, "@app/pp/x.styles", false);
        assert!(
            missing.is_none(),
            "a miss of the selected pattern must not try another pattern, got {missing:?}"
        );
        let targets = nearest_tsconfig_alias_targets(ctx, &from_file, "@app/x.styles");
        assert!(
            targets
                .iter()
                .all(|target| target.starts_with(root.join("a"))),
            "only the targets of the selected pattern get credit, got {targets:?}"
        );
    });
}

#[test]
fn path_alias_capture_takes_the_text_at_the_wildcard() {
    assert_eq!(
        path_alias_capture("@/*", "@/components/Button"),
        Some("components/Button")
    );
    assert_eq!(
        path_alias_capture("@app/*/test", "@app/foo/bar/test"),
        Some("foo/bar")
    );
    assert_eq!(path_alias_capture("@/*", "@"), None);
    assert_eq!(path_alias_capture("*", "shared"), Some("shared"));
    assert_eq!(path_alias_capture("$lib", "$lib"), Some(""));
    assert_eq!(path_alias_capture("$lib", "$lib/utils"), None);
}

#[test]
fn path_alias_specificity_ranks_exact_then_prefix_length() {
    assert_eq!(path_alias_specificity("$lib"), usize::MAX);
    assert_eq!(path_alias_specificity("@app/*"), "@app/".len());
    assert_eq!(path_alias_specificity("@a*.styles"), "@a".len());
    assert_eq!(path_alias_specificity("*"), 0);
}

#[test]
fn select_path_alias_prefers_exact_then_longest_prefix_then_first() {
    let patterns: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
        r#"{"*":["any/*"],"@app/*":["first/*"],"@app/*.ts":["second/*"],"@app/x":["exact"]}"#,
    )
    .unwrap();
    let pick =
        |specifier: &'static str| select_path_alias(&patterns, specifier).map(|(p, c, _)| (p, c));
    assert_eq!(pick("@app/x"), Some(("@app/x", "")));
    assert_eq!(pick("@app/y.ts"), Some(("@app/*", "y.ts")));
    assert_eq!(pick("lodash"), Some(("*", "lodash")));
}
