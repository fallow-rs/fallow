//! Issue #3208: package entries under `lib/` map back to `src/` when the build
//! output is missing, and a `lib/` file on disk always stays the entry.

use std::path::Path;

use super::common::{create_config, fixture_path};

fn relative_paths<'a>(root: &Path, paths: impl Iterator<Item = &'a Path>) -> Vec<String> {
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut relative: Vec<String> = paths
        .filter_map(|path| {
            path.strip_prefix(root)
                .or_else(|_| path.strip_prefix(&canonical_root))
                .ok()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
        })
        .collect();
    relative.sort();
    relative
}

fn unused_file_paths(root: &Path) -> Vec<String> {
    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");
    relative_paths(
        root,
        results.unused_files.iter().map(|f| f.file.path.as_path()),
    )
}

#[test]
fn missing_lib_output_maps_package_entries_and_imports_to_source() {
    let root = fixture_path("issue-3208-lib-output-source");
    let results =
        fallow_core::analyze(&create_config(root.clone())).expect("analysis should succeed");

    assert_eq!(
        relative_paths(
            &root,
            results.unused_files.iter().map(|f| f.file.path.as_path())
        ),
        vec!["packages/consumer/src/helper.ts".to_string()],
        "uses-lib source must be reachable through its lib/ entries; only the file behind \
         the unresolved relative lib/ import stays unused"
    );

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|i| i.import.specifier.as_str())
        .collect();
    assert_eq!(
        unresolved,
        vec!["../lib/helper.js"],
        "the package import through the exports map resolves; a relative lib/ import to a \
         missing file stays unresolved"
    );

    assert!(
        results.unlisted_dependencies.is_empty(),
        "workspace import must not be an unlisted dependency, found: {:?}",
        results
            .unlisted_dependencies
            .iter()
            .map(|d| &d.dep.package_name)
            .collect::<Vec<_>>()
    );
}

fn write_lib_package(root: &Path, entry_main: &str) {
    std::fs::write(
        root.join("package.json"),
        format!(r#"{{"name":"hand-written","main":"{entry_main}"}}"#),
    )
    .expect("package manifest");
    std::fs::create_dir_all(root.join("lib")).expect("lib directory");
    std::fs::create_dir_all(root.join("src")).expect("src directory");
    std::fs::write(
        root.join("lib/index.js"),
        "const util = require('./util');\nmodule.exports = util;\n",
    )
    .expect("hand-written lib entry");
    std::fs::write(root.join("lib/util.js"), "module.exports = { value: 1 };\n")
        .expect("hand-written lib helper");
    std::fs::write(root.join("src/index.ts"), "export const unrelated = 1;\n")
        .expect("unrelated source file");
}

#[test]
fn hand_written_lib_entry_on_disk_stays_the_entry() {
    for entry_main in ["./lib/index.js", "./lib/index"] {
        let directory = tempfile::tempdir().expect("temporary project directory");
        let root = directory.path();
        write_lib_package(root, entry_main);

        assert_eq!(
            unused_file_paths(root),
            vec!["src/index.ts".to_string()],
            "lib/ files on disk stay reachable from main={entry_main}; the unrelated src file \
             keeps its current result"
        );
    }
}

#[test]
fn hand_written_lib_entry_does_not_fall_back_to_source_index() {
    let directory = tempfile::tempdir().expect("temporary project directory");
    let root = directory.path();
    write_lib_package(root, "./lib/util.js");
    std::fs::write(root.join("index.js"), "module.exports = {};\n").expect("root index");

    let unused = unused_file_paths(root);
    assert!(
        !unused.contains(&"lib/util.js".to_string()),
        "the hand-written lib/util.js entry must stay the entry, unused: {unused:?}"
    );
    assert!(
        unused.contains(&"src/index.ts".to_string()),
        "the index fallback must not replace the lib/ entry, unused: {unused:?}"
    );
}
