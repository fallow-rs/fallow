use super::common::{create_config, fixture_path};

/// An Nx app has a solution-style `tsconfig.json` (`files: []`) that extends
/// the base config and references `tsconfig.app.json`. The app config owns
/// the source files, so its own `paths` decide the aliases, and the `paths`
/// that the solution config inherits from the base config do not hide them.
#[test]
fn referenced_owner_config_paths_win_over_the_solution_config() {
    let root = fixture_path("tsconfig-solution-owner-paths");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "every alias must resolve: {unresolved:?}"
    );

    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused
            .iter()
            .any(|path| path.ends_with("apps/web/src/lib/util.ts")),
        "the aliased file is used: {unused:?}"
    );
    assert!(
        !unused
            .iter()
            .any(|path| path.ends_with("libs/shared/src/b.ts")),
        "the shared file is used: {unused:?}"
    );
}
