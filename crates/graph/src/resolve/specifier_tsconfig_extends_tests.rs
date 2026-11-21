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

/// The nearest config contains the file, so its own `paths` win over the
/// `paths` of a referenced config that also contains the file.
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn nearest_config_that_contains_the_file_keeps_its_own_paths() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/index.ts"), "").unwrap();
    fs::write(root.join("src/near.ts"), "export {};").unwrap();
    fs::write(root.join("src/other.ts"), "export {};").unwrap();
    fs::write(
        root.join("tsconfig.json"),
        r#"{"include":["src"],"compilerOptions":{"paths":{"@x":["./src/near.ts"]}},"references":[{"path":"./tsconfig.other.json"}]}"#,
    )
    .unwrap();
    fs::write(
        root.join("tsconfig.other.json"),
        r#"{"include":["src"],"compilerOptions":{"paths":{"@x":["./src/other.ts"]}}}"#,
    )
    .unwrap();
    let from_file = root.join("src/index.ts");
    let near = root.join("src/near.ts");
    let other = root.join("src/other.ts");
    let files = [(near.as_path(), FileId(1)), (other.as_path(), FileId(2))];

    with_ctx_at_root_and_files(root, &files, |ctx| {
        let result = super::try_nearest_tsconfig_path_alias(ctx, &from_file, "@x", false);
        assert!(
            matches!(result, Some(ResolveResult::InternalModule(FileId(1)))),
            "got {result:?}"
        );
    });
}

/// A solution-style nearest config (`files: []`) with `extends` does not
/// contain the file. The `rootDirs` that it inherits do not count, and the
/// `rootDirs` of the referenced config that owns the file do.
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn solution_config_rootdirs_give_way_to_the_owner() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    for folder in ["src", "generated", "wrong"] {
        fs::create_dir_all(root.join(folder)).unwrap();
    }
    fs::write(root.join("src/index.ts"), "").unwrap();
    fs::write(root.join("generated/types.ts"), "export {};").unwrap();
    fs::write(
        root.join("base.json"),
        r#"{"compilerOptions":{"rootDirs":["./src","./wrong"]}}"#,
    )
    .unwrap();
    fs::write(
        root.join("tsconfig.json"),
        r#"{"extends":"./base.json","files":[],"references":[{"path":"./tsconfig.app.json"}]}"#,
    )
    .unwrap();
    fs::write(
        root.join("tsconfig.app.json"),
        r#"{"include":["src"],"compilerOptions":{"rootDirs":["./src","./generated"]}}"#,
    )
    .unwrap();
    let from_file = root.join("src/index.ts");
    let generated = root.join("generated/types.ts");
    let files = [(generated.as_path(), FileId(1))];

    with_ctx_at_root_and_files(root, &files, |ctx| {
        let result = super::try_tsconfig_root_dirs(ctx, &from_file, "./types");
        assert!(
            matches!(result, Some(ResolveResult::InternalModule(FileId(1)))),
            "got {result:?}"
        );
    });
}

/// `files` and `include` add up, as in TypeScript. A referenced config that
/// inherits `files: []` from the solution config it extends, and sets its
/// own `include`, contains the files that `include` matches.
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn inherited_empty_files_and_own_include_contain_the_file() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/index.ts"), "").unwrap();
    fs::write(root.join("src/target.ts"), "export {};").unwrap();
    fs::write(
        root.join("tsconfig.json"),
        r#"{"files":[],"references":[{"path":"./tsconfig.app.json"}]}"#,
    )
    .unwrap();
    fs::write(
        root.join("tsconfig.app.json"),
        r#"{"extends":"./tsconfig.json","include":["src"],"compilerOptions":{"paths":{"@t":["./src/target.ts"]}}}"#,
    )
    .unwrap();
    let from_file = root.join("src/index.ts");
    let target = root.join("src/target.ts");
    let files = [(target.as_path(), FileId(1))];

    with_ctx_at_root_and_files(root, &files, |ctx| {
        let result = super::try_nearest_tsconfig_path_alias(ctx, &from_file, "@t", false);
        assert!(
            matches!(result, Some(ResolveResult::InternalModule(FileId(1)))),
            "got {result:?}"
        );
    });
}

/// A config inherits `baseUrl` from a base config in another folder and
/// sets its own `paths`. TypeScript 5 resolves the targets against
/// `baseUrl`, TypeScript 7 against the folder of the config that sets
/// `paths`. A target that exists under either folder resolves.
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn paths_target_resolves_against_base_url_or_the_paths_config() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("app/src/lib")).unwrap();
    fs::write(root.join("app/src/index.ts"), "").unwrap();
    fs::write(root.join("app/src/lib/util.ts"), "export {};").unwrap();
    fs::write(
        root.join("base.json"),
        r#"{"compilerOptions":{"baseUrl":"."}}"#,
    )
    .unwrap();
    fs::write(
        root.join("app/tsconfig.json"),
        r#"{"extends":"../base.json","compilerOptions":{"paths":{"~/*":["./src/lib/*"]}}}"#,
    )
    .unwrap();
    let from_file = root.join("app/src/index.ts");
    let util = root.join("app/src/lib/util.ts");
    let files = [(util.as_path(), FileId(1))];

    with_ctx_at_root_and_files(root, &files, |ctx| {
        let result = super::try_nearest_tsconfig_path_alias(ctx, &from_file, "~/util", false);
        assert!(
            matches!(result, Some(ResolveResult::InternalModule(FileId(1)))),
            "got {result:?}"
        );
    });
}
