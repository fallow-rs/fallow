//! Commands in project files that are not package.json scripts name script
//! files and binaries: the lifecycle commands of a development container
//! (`.devcontainer/devcontainer.json`), the scripts of a `t3.json` project
//! file, and a CI step that puts a `node_modules/.bin` path in a shell
//! variable before it runs the binary.

use super::common::{create_config, fixture_path};

#[cfg_attr(miri, ignore)]
#[test]
fn command_files_reference_scripts_and_binaries() {
    let root = fixture_path("command-file-references");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&f.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    unused_files.sort();
    assert_eq!(
        unused_files,
        ["scripts/orphan.mjs"],
        "each script that a devcontainer command, a shell file of such a command, or a t3.json script runs is in use"
    );

    let unused_dev: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert_eq!(
        unused_dev,
        ["unused-tool"],
        "a CI step that assigns node_modules/.bin/bundler-tool to a variable uses the package"
    );
}
