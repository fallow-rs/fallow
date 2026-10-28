//! Issue #3967: the arguments after the script of a runtime are data. They
//! name no entry point. A program that fallow does not know keeps its
//! literal file arguments, but no glob expands from them. A program that
//! runs every file it gets, such as `mocha`, keeps its file arguments as
//! entry points.

use super::common::{create_config, fixture_path};

#[test]
fn script_arguments_are_data_not_entry_points() {
    let root = fixture_path("issue-3967-script-argument-entries");
    let results =
        fallow_core::analyze(&create_config(root.clone())).expect("analysis should succeed");

    let mut unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(&root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    unused_files.sort();
    // `src/lib/b.js` matches the glob after `node tool.js`. `src/lib/flag.js`
    // is the value of `--input` after the script. `src/lib/helper-arg.js` is
    // an argument after the script in a shell helper. `src/lib/watched.js`
    // matches the glob of `watchee -f`, and fallow expands no glob for a
    // program that it does not know. The runtime scripts `tool.js` and
    // `api-docs/build-openapi.js`, the `mocha` file `checks/run.js`, and the
    // literal `jscodeshift -t` value `codemods/transform.js` stay entry
    // points, so `src/lib/spec-target.js` and `src/lib/codemod-helper.js`
    // are reachable.
    assert_eq!(
        unused_files,
        vec![
            "src/lib/b.js".to_string(),
            "src/lib/flag.js".to_string(),
            "src/lib/helper-arg.js".to_string(),
            "src/lib/watched.js".to_string(),
        ]
    );

    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|finding| finding.export.export_name.clone())
        .collect();
    assert!(
        unused_exports.contains(&"unused".to_string()),
        "`src/a.js` is not an entry point, so its unused export is reported: {unused_exports:?}"
    );
}
