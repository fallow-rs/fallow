//! A path that code builds from the directory of the current module names a
//! project file: `path.join(__dirname, "../scripts/agent.mjs")`. Code hands
//! the path to a child process, a file read, or a dynamic `import()` of a
//! file URL, so the file is in use. Only the `import()` form loads the module
//! and credits its exports. A directory path is not a reference to the index
//! file of that directory. A target that is not on disk, such as build
//! output, is not an unresolved import.

use super::common::{create_config, fixture_path};

fn relative(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg_attr(miri, ignore)]
#[test]
fn module_dir_joins_reference_script_files() {
    let root = fixture_path("module-dir-path-references");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| relative(&f.file.path, &root))
        .collect();
    unused_files.sort();
    assert_eq!(
        unused_files,
        ["scripts/orphan.mjs", "src/lib/index.js"],
        "only the file with no reference and the index file of a joined directory stay unused"
    );

    let unused_exports: Vec<(String, &str)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                relative(&e.export.path, &root),
                e.export.export_name.as_str(),
            )
        })
        .filter(|(file, _)| file.starts_with("scripts/"))
        .collect();
    assert_eq!(
        unused_exports,
        [("scripts/agent.mjs".to_string(), "agent")],
        "a path that code only passes on credits no export, and an import() of the path credits the module"
    );

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "a path to a file that is not on disk must not become an unresolved import, got: {unresolved:?}"
    );
}
