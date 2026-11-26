//! The `deep-modules` preset and the `presetPublicFiles` opt-in.

use super::*;

fn preset(preset: BoundaryPreset, preset_public_files: bool) -> BoundaryConfig {
    BoundaryConfig {
        preset: Some(preset),
        preset_public_files,
        ..BoundaryConfig::default()
    }
}

fn project_with_dirs(dirs: &[&str]) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for dir in dirs {
        std::fs::create_dir_all(temp.path().join(dir)).unwrap();
    }
    temp
}

fn zone_summary(config: &BoundaryConfig) -> Vec<(String, Vec<String>, Vec<String>)> {
    config
        .zones
        .iter()
        .map(|zone| {
            (
                zone.name.clone(),
                zone.patterns.clone(),
                zone.public_files.clone(),
            )
        })
        .collect()
}

#[test]
fn deep_modules_orders_module_children_before_the_catch_all_zones() {
    let temp = project_with_dirs(&["src/modules/billing", "src/modules/orders"]);
    let mut config = preset(BoundaryPreset::DeepModules, false);
    expand_boundary_preset(&mut config, temp.path());
    let _groups = config.expand_auto_discover(temp.path());

    let index = "index.{ts,tsx,mts,cts,js,jsx,mjs,cjs}".to_owned();
    assert_eq!(
        zone_summary(&config),
        vec![
            (
                "modules/billing".to_owned(),
                vec!["src/modules/billing/**".to_owned()],
                vec![format!("src/modules/billing/{index}")],
            ),
            (
                "modules/orders".to_owned(),
                vec!["src/modules/orders/**".to_owned()],
                vec![format!("src/modules/orders/{index}")],
            ),
            (
                "modules".to_owned(),
                vec!["src/modules/**".to_owned()],
                vec![],
            ),
            ("app".to_owned(), vec!["src/**".to_owned()], vec![]),
            (
                "tests".to_owned(),
                vec![
                    "test/**".to_owned(),
                    "tests/**".to_owned(),
                    "e2e/**".to_owned()
                ],
                vec![],
            ),
        ]
    );
    assert!(config.rules.is_empty(), "the preset has no rules");
}

#[test]
fn deep_modules_uses_the_features_folder_when_modules_is_missing() {
    let temp = project_with_dirs(&["src/features/auth"]);
    let mut config = preset(BoundaryPreset::DeepModules, false);
    expand_boundary_preset(&mut config, temp.path());
    assert_eq!(config.zones[0].auto_discover, vec!["src/features"]);
    assert_eq!(config.zones[0].patterns, vec!["src/features/**"]);
}

#[test]
fn deep_modules_without_a_module_folder_is_an_error() {
    let temp = project_with_dirs(&["src"]);
    let errors = preset(BoundaryPreset::DeepModules, false).validate_preset(temp.path());
    assert_eq!(
        errors,
        vec![ZoneValidationError::PresetModuleRootMissing(vec![
            "src/modules".to_owned(),
            "src/features".to_owned(),
        ])]
    );
    assert!(
        errors[0]
            .to_string()
            .contains("'src/modules', 'src/features'")
    );

    let temp = project_with_dirs(&["src/features"]);
    assert!(
        preset(BoundaryPreset::DeepModules, false)
            .validate_preset(temp.path())
            .is_empty()
    );
}

#[test]
fn deep_modules_watches_both_candidate_folders() {
    let temp = project_with_dirs(&["src/features/auth"]);
    let dirs = preset(BoundaryPreset::DeepModules, false).auto_discover_dirs(temp.path());
    assert!(dirs.contains(&temp.path().join("src/modules")), "{dirs:?}");
    assert!(dirs.contains(&temp.path().join("src/features")), "{dirs:?}");
}

#[test]
fn preset_public_files_needs_a_supporting_preset() {
    let temp = project_with_dirs(&["src/modules"]);
    let without_preset = BoundaryConfig {
        preset_public_files: true,
        ..BoundaryConfig::default()
    };
    assert_eq!(
        without_preset.validate_preset(temp.path()),
        vec![ZoneValidationError::PresetPublicFilesUnsupported(None)]
    );
    for unsupported in [BoundaryPreset::Layered, BoundaryPreset::Hexagonal] {
        let name = unsupported.name().to_owned();
        assert_eq!(
            preset(unsupported, true).validate_preset(temp.path()),
            vec![ZoneValidationError::PresetPublicFilesUnsupported(Some(
                name
            ))]
        );
    }
    for supported in [
        BoundaryPreset::Bulletproof,
        BoundaryPreset::FeatureSliced,
        BoundaryPreset::DeepModules,
    ] {
        assert!(
            preset(supported, true)
                .validate_preset(temp.path())
                .is_empty()
        );
    }
}

#[test]
fn bulletproof_and_feature_sliced_have_no_public_files_by_default() {
    for kind in [BoundaryPreset::Bulletproof, BoundaryPreset::FeatureSliced] {
        let mut config = preset(kind, false);
        config.expand("src");
        assert!(
            config.zones.iter().all(|zone| zone.public_files.is_empty()),
            "{:?}",
            zone_summary(&config)
        );
    }
}

#[test]
fn bulletproof_opt_in_gives_each_feature_its_index_file() {
    let mut config = preset(BoundaryPreset::Bulletproof, true);
    config.expand("src");
    let features = config.zones.iter().find(|z| z.name == "features").unwrap();
    assert_eq!(
        features.public_files,
        vec!["index.{ts,tsx,mts,cts,js,jsx,mjs,cjs}"]
    );
    assert!(
        config
            .zones
            .iter()
            .filter(|z| z.name != "features")
            .all(|z| z.public_files.is_empty())
    );
}

#[test]
fn feature_sliced_opt_in_gives_each_slice_its_public_api() {
    let mut config = preset(BoundaryPreset::FeatureSliced, true);
    config.expand("src");
    let public: Vec<(&str, Vec<String>)> = config
        .zones
        .iter()
        .map(|z| (z.name.as_str(), z.public_files.clone()))
        .collect();
    let index = "index.{ts,tsx,mts,cts,js,jsx,mjs,cjs}";
    assert_eq!(
        public,
        vec![
            ("app", vec![]),
            ("pages", vec![format!("src/pages/*/{index}")]),
            ("widgets", vec![format!("src/widgets/*/{index}")]),
            ("features", vec![format!("src/features/*/{index}")]),
            (
                "entities",
                vec![
                    format!("src/entities/*/{index}"),
                    "src/entities/*/@x/*".to_owned()
                ]
            ),
            ("shared", vec![]),
        ]
    );
}

#[test]
fn preset_public_files_round_trips_and_defaults_to_false() {
    let config: BoundaryConfig =
        serde_json::from_str(r#"{ "preset": "deep-modules", "presetPublicFiles": true }"#).unwrap();
    assert_eq!(config.preset, Some(BoundaryPreset::DeepModules));
    assert!(config.preset_public_files);
    let json = serde_json::to_value(&config).unwrap();
    assert_eq!(json["presetPublicFiles"], true);

    let config: BoundaryConfig = serde_json::from_str(r#"{ "preset": "bulletproof" }"#).unwrap();
    assert!(!config.preset_public_files);
    assert!(
        serde_json::to_value(&config)
            .unwrap()
            .get("presetPublicFiles")
            .is_none()
    );
}

#[test]
fn deep_modules_puts_its_catch_all_zones_after_the_user_zones() {
    let temp = project_with_dirs(&["src/modules/billing"]);
    let mut config = BoundaryConfig {
        zones: vec![BoundaryZone {
            name: "shared".to_owned(),
            patterns: vec!["src/shared/**".to_owned()],
            ..BoundaryZone::default()
        }],
        ..preset(BoundaryPreset::DeepModules, false)
    };
    expand_boundary_preset(&mut config, temp.path());
    let names: Vec<&str> = config.zones.iter().map(|z| z.name.as_str()).collect();
    assert_eq!(names, vec!["modules", "shared", "app", "tests"]);
}

#[test]
fn deep_modules_prefers_a_candidate_with_module_folders() {
    let temp = project_with_dirs(&["src/modules", "src/features/auth"]);
    let mut config = preset(BoundaryPreset::DeepModules, false);
    expand_boundary_preset(&mut config, temp.path());
    assert_eq!(config.zones[0].auto_discover, vec!["src/features"]);

    let temp = project_with_dirs(&["src/modules", "src/features"]);
    let mut config = preset(BoundaryPreset::DeepModules, false);
    expand_boundary_preset(&mut config, temp.path());
    assert_eq!(config.zones[0].auto_discover, vec!["src/modules"]);
}

#[test]
fn deep_modules_uses_the_tsconfig_root_dir() {
    let temp = project_with_dirs(&["lib/modules/billing"]);
    std::fs::write(
        temp.path().join("tsconfig.json"),
        r#"{ "compilerOptions": { "rootDir": "lib" } }"#,
    )
    .unwrap();
    let mut config = preset(BoundaryPreset::DeepModules, false);
    assert!(config.validate_preset(temp.path()).is_empty());
    expand_boundary_preset(&mut config, temp.path());
    assert_eq!(config.zones[0].auto_discover, vec!["lib/modules"]);
    assert_eq!(config.zones[1].patterns, vec!["lib/**"]);
}

#[test]
fn a_user_modules_zone_needs_no_module_folder() {
    let temp = project_with_dirs(&["src/domains/billing"]);
    let config = BoundaryConfig {
        zones: vec![BoundaryZone {
            name: "modules".to_owned(),
            patterns: vec!["src/domains/**".to_owned()],
            auto_discover: vec!["src/domains".to_owned()],
            public_files: vec!["index.ts".to_owned()],
            ..BoundaryZone::default()
        }],
        ..preset(BoundaryPreset::DeepModules, false)
    };
    assert!(config.validate_preset(temp.path()).is_empty());
}
