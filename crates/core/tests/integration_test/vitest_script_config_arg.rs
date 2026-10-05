//! A package script that runs `vitest --config <file>` points vitest at a
//! config file that no config file pattern matches. The vitest plugin must
//! read that file, so the test files that its `test.include` selects are
//! entry points, and the default export of the config is used. The script
//! config adds to the default `vitest.config.ts` and does not replace it.

use super::common::{create_config, fixture_path};

fn relative(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn script_config_arg_includes_become_entry_points() {
    let root = fixture_path("vitest-script-config-arg");
    let mut config = create_config(root);
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| relative(&file.file.path, &config.root))
        .collect();
    unused_files.sort();
    assert_eq!(
        unused_files,
        vec!["packages/widgets/checks/orphan.ts", "snaps/orphan.ts"],
        "files that the script config or the default config includes must be \
         entry points; files outside both includes must stay unused"
    );

    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|export| {
            format!(
                "{}:{}",
                relative(&export.export.path, &config.root),
                export.export.export_name
            )
        })
        .collect();
    assert_eq!(
        unused_exports,
        vec!["vitest.snap.config.ts:snapshotDir".to_string()],
        "the default export of a script config must be used; \
         its other exports must stay unused"
    );
}
