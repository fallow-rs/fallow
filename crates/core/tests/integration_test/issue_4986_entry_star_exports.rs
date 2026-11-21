use super::common::{create_config, fixture_path};

const FIXTURE: &str = "issue-4986-entry-star-exports";

fn sorted_names(exports: &[fallow_types::output_dead_code::UnusedExportFinding]) -> Vec<String> {
    let mut names: Vec<String> = exports
        .iter()
        .map(|finding| finding.export.export_name.clone())
        .collect();
    names.sort_unstable();
    names
}

fn sorted_type_names(types: &[fallow_types::output_dead_code::UnusedTypeFinding]) -> Vec<String> {
    let mut names: Vec<String> = types
        .iter()
        .map(|finding| finding.export.export_name.clone())
        .collect();
    names.sort_unstable();
    names
}

fn config_from_fixture_file() -> fallow_config::ResolvedConfig {
    let root = fixture_path(FIXTURE);
    let (loaded, _path) = fallow_config::FallowConfig::find_and_load(&root)
        .expect("config load")
        .expect("fixture has .fallowrc.json");
    loaded.resolve(
        root,
        fallow_config::OutputFormat::Human,
        1,
        true,
        true,
        None,
    )
}

#[test]
fn star_re_exports_of_an_entry_file_stay_credited_by_default() {
    let config = create_config(fixture_path(FIXTURE));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // A local export of the entry file hides the `shadowed` of `a.ts`, so
    // that declaration does not reach the entry file with or without the
    // option.
    assert_eq!(sorted_names(&results.unused_exports), ["shadowed"]);
    assert!(
        results.unused_exports[0]
            .export
            .path
            .ends_with("packages/lib/src/a.ts")
    );
    assert_eq!(
        sorted_type_names(&results.unused_types),
        Vec::<String>::new()
    );
}

#[test]
fn workspace_setting_reports_unused_names_behind_an_entry_star() {
    let config = config_from_fixture_file();
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // `subUnused` is behind the `export *` of a subpath entry (`@p/lib/sub`).
    // `shadowed` is a local export of the entry file and an export of
    // `a.ts`. The local export hides the name of the star source, so each of
    // the two has no importer.
    // `pubUnused` is in `@p/pub`, which `publicPackages` names.
    // `plainUnused` is in a workspace that the `packages/*` key does not
    // select. `shared` comes from two star sources, so the entry file exports
    // no binding under that name. `ns` has a consumer, and its members are a
    // namespace object, not names of the entry file.
    assert_eq!(
        sorted_names(&results.unused_exports),
        [
            "chainUnused",
            "deepUnused",
            "oneUnused",
            "shadowed",
            "shadowed",
            "subUnused",
            "unusedStar"
        ]
    );
    assert_eq!(
        sorted_type_names(&results.unused_types),
        ["UnusedShape", "UnusedType"]
    );

    let unused_star = results
        .unused_exports
        .iter()
        .find(|finding| finding.export.export_name == "unusedStar")
        .expect("unusedStar is reported");
    assert!(
        unused_star.export.path.ends_with("packages/lib/src/a.ts"),
        "the finding is at the declaration, found {:?}",
        unused_star.export.path
    );
    assert!(!unused_star.export.is_re_export);
}

#[test]
fn root_option_reports_unused_names_behind_every_entry_star() {
    // This config does not read the fixture file, so no package is public.
    let mut config = create_config(fixture_path(FIXTURE));
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        sorted_names(&results.unused_exports),
        [
            "chainUnused",
            "deepUnused",
            "oneUnused",
            "plainUnused",
            "pubUnused",
            "shadowed",
            "shadowed",
            "subUnused",
            "unusedStar"
        ]
    );
    assert_eq!(
        sorted_type_names(&results.unused_types),
        ["UnusedShape", "UnusedType"]
    );
}

#[test]
fn ignore_exports_rule_on_the_entry_file_covers_names_behind_its_star() {
    let root = fixture_path(FIXTURE);
    let (mut loaded, _path) = fallow_config::FallowConfig::find_and_load(&root)
        .expect("config load")
        .expect("fixture has .fallowrc.json");
    loaded.ignore_exports.push(fallow_config::IgnoreExportRule {
        file: "packages/lib/src/index.ts".to_owned(),
        exports: vec![
            "unusedStar".to_owned(),
            "deepUnused".to_owned(),
            "shadowed".to_owned(),
        ],
    });
    let config = loaded.resolve(
        root,
        fallow_config::OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // The rule names the local `shadowed` of the entry file. The `shadowed`
    // of `a.ts` does not reach the entry file, so it stays reported.
    assert_eq!(
        sorted_names(&results.unused_exports),
        ["chainUnused", "oneUnused", "shadowed", "subUnused"]
    );
    let shadowed = results
        .unused_exports
        .iter()
        .find(|finding| finding.export.export_name == "shadowed")
        .expect("shadowed is reported");
    assert!(shadowed.export.path.ends_with("packages/lib/src/a.ts"));
}
