use super::common::{create_config, fixture_path};

/// Analyze `fixture` and check that every alias of `apps/web/src/main.ts`
/// resolves and that neither aliased file is unused.
fn assert_aliases_resolve(fixture: &str) {
    let config = create_config(fixture_path(fixture));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "{fixture}: every alias must resolve: {unresolved:?}"
    );

    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    for used in ["apps/web/src/lib/util.ts", "libs/shared/src/b.ts"] {
        assert!(
            !unused.iter().any(|path| path.ends_with(used)),
            "{fixture}: {used} is used: {unused:?}"
        );
    }
}

/// An Nx app on TypeScript 5 has a solution-style `tsconfig.json`
/// (`files: []`) that extends the base config and references
/// `tsconfig.app.json`. The app config owns the source files, so its own
/// `paths` decide the aliases, against the `baseUrl` of the base config.
/// The `paths` that the solution config inherits do not hide them.
#[test]
fn referenced_owner_config_paths_win_over_the_solution_config() {
    assert_aliases_resolve("tsconfig-solution-owner-paths");
}

/// The same layout without `baseUrl`, as TypeScript 7 requires: each
/// `paths` target resolves against the folder of the config that sets
/// `paths`.
#[test]
fn paths_without_base_url_resolve_against_their_config_folder() {
    assert_aliases_resolve("tsconfig-paths-config-folder");
}
