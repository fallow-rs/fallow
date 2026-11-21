//! The tsconfig fallbacks of the resolver read every `extends` shape through
//! the merged tsconfig reader.

use std::fs;

use fallow_config::TsconfigExtendsShape;
use fallow_types::discover::FileId;
use tempfile::tempdir;

use super::tests::with_ctx_at_root_and_files;
use crate::resolve::ResolveResult;
use crate::resolve::base_url_directory::nearest_base_url_dir;
use crate::resolve::package_credit::nearest_tsconfig_alias_targets;

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn every_extends_shape_gives_paths_and_base_url() {
    for shape in TsconfigExtendsShape::ALL {
        let temp = tempdir().unwrap();
        let root = temp.path();
        shape.write(root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/index.ts"), "").unwrap();
        fs::write(root.join("src/clock.ts"), "export const now = 1;\n").unwrap();
        let from_file = root.join("src/index.ts");
        let clock = root.join("src/clock.ts");
        let files = [(clock.as_path(), FileId(1))];

        with_ctx_at_root_and_files(root, &files, |ctx| {
            let name = shape.name();
            assert!(
                super::matches_nearest_tsconfig_path_alias(ctx, &from_file, "@app/clock"),
                "{name}: paths pattern"
            );
            let result =
                super::try_nearest_tsconfig_path_alias(ctx, &from_file, "@app/clock", false);
            assert!(
                matches!(result, Some(ResolveResult::InternalModule(FileId(1)))),
                "{name}: paths target, got {result:?}"
            );
            assert_eq!(
                nearest_base_url_dir(ctx, &from_file),
                Some(root.to_path_buf()),
                "{name}: baseUrl"
            );
            assert_eq!(
                nearest_tsconfig_alias_targets(ctx, &from_file, "@app/clock").first(),
                Some(&root.join("src/clock")),
                "{name}: alias targets"
            );
        });
    }
}

/// A referenced config gets its `include` from a shared package config that
/// writes it with `${configDir}`. The pattern is relative to the referenced
/// config, so the config applies to its files and its `paths` count.
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn referenced_config_applies_through_an_inherited_config_dir_include() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("node_modules/@acme/tsconfig")).unwrap();
    fs::write(
        root.join("node_modules/@acme/tsconfig/package.json"),
        r#"{"name":"@acme/tsconfig","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(
        root.join("node_modules/@acme/tsconfig/tsconfig.json"),
        r#"{"include":["${configDir}/src"]}"#,
    )
    .unwrap();
    fs::write(
        root.join("tsconfig.json"),
        r#"{"files":[],"references":[{"path":"./tsconfig.app.json"}]}"#,
    )
    .unwrap();
    fs::write(
        root.join("tsconfig.app.json"),
        r#"{"extends":"@acme/tsconfig","compilerOptions":{"paths":{"@lib/*":["./src/*"]}}}"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/index.ts"), "").unwrap();
    let from_file = root.join("src/index.ts");

    with_ctx_at_root_and_files(root, &[], |ctx| {
        assert!(super::matches_nearest_tsconfig_path_alias(
            ctx,
            &from_file,
            "@lib/util"
        ));
    });
}
