//! A workspace package script runs Jest through Yarn as `run -T jest` or
//! `yarn run -T jest`, with Jest declared only in the root package. The
//! `--config` file of each call must reach the Jest plugin, so its setup
//! files and its `<rootDir>` `moduleNameMapper` targets are used.

use super::common::{create_config, fixture_path};

fn relative(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn yarn_top_level_jest_config_and_module_name_mapper_are_credited() {
    let root = fixture_path("yarn-run-top-level-jest");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| relative(&file.file.path, &root))
        .collect();
    unused_files.sort();
    assert_eq!(
        unused_files,
        vec!["packages/web/src/orphan.js"],
        "setup files and moduleNameMapper targets of a `run -T jest --config` \
         call must be used; an unrelated file must stay unused"
    );

    let unused_dev: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dev.contains(&"jest"),
        "`run -T jest` in a workspace script uses the root `jest`, unused dev dependencies: {unused_dev:?}"
    );
}
