//! Bounded work for repeated `autoDiscover` folders and repeated zone
//! declarations.

use std::time::{Duration, Instant};

use super::*;

/// The wall-clock bound of one bounded expansion in a debug build.
const EXPAND_BOUND: Duration = Duration::from_secs(3);

fn project_with_children(parent: &str, children: usize) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    for index in 0..children {
        std::fs::create_dir_all(temp.path().join(format!("{parent}/c{index}"))).unwrap();
    }
    temp
}

fn auto_discover_zone(name: &str, folders: Vec<String>) -> BoundaryZone {
    BoundaryZone {
        name: name.to_owned(),
        auto_discover: folders,
        ..BoundaryZone::default()
    }
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn spellings_of_one_auto_discover_folder_give_one_pattern_per_child() {
    const CHILDREN: usize = 50;
    let temp = project_with_children("src/d", CHILDREN);
    let mut spellings = vec![
        "src/d".to_owned(),
        "src//d".to_owned(),
        "./src/d/".to_owned(),
    ];
    spellings.extend((1..200).map(|dots| format!("src/{}d", "./".repeat(dots))));
    let mut config = BoundaryConfig {
        zones: vec![auto_discover_zone("f", spellings)],
        ..BoundaryConfig::default()
    };

    config.expand_auto_discover(temp.path());

    assert_eq!(config.zones.len(), CHILDREN);
    for zone in &config.zones {
        assert_eq!(zone.patterns.len(), 1, "{}", zone.name);
    }
    assert_eq!(config.zones[0].patterns, vec!["src/d/c0/**".to_owned()]);
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn repeated_auto_discover_zone_declarations_expand_rules_once() {
    const CHILDREN: usize = 40;
    const DECLARATIONS: usize = 24;
    let temp = project_with_children("features", CHILDREN);
    let mut zones = Vec::new();
    for _ in 0..DECLARATIONS {
        zones.push(auto_discover_zone("app", vec!["features".to_owned()]));
        zones.push(auto_discover_zone("lib", vec!["features".to_owned()]));
    }
    let mut config = BoundaryConfig {
        zones,
        rules: vec![BoundaryRule {
            from: "app".to_owned(),
            allow: vec!["lib".to_owned(); 200],
            allow_type_only: vec![],
        }],
        ..BoundaryConfig::default()
    };

    let start = Instant::now();
    config.expand_auto_discover(temp.path());
    let elapsed = start.elapsed();

    let app_rules: Vec<&BoundaryRule> = config
        .rules
        .iter()
        .filter(|rule| rule.from.starts_with("app/"))
        .collect();
    assert_eq!(app_rules.len(), CHILDREN);
    assert_eq!(app_rules[0].allow.len(), CHILDREN);
    assert!(elapsed < EXPAND_BOUND, "expansion took {elapsed:?}");
}
