//! Zone roots and `autoDiscover` directories that leave the project root.

use super::*;

fn zone(root: Option<&str>, auto_discover: &str) -> BoundaryConfig {
    BoundaryConfig {
        zones: vec![BoundaryZone {
            name: "features".to_string(),
            root: root.map(str::to_owned),
            auto_discover: vec![auto_discover.to_owned()],
            ..BoundaryZone::default()
        }],
        ..BoundaryConfig::default()
    }
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn a_zone_root_that_normalizes_to_an_absolute_path_lists_nothing() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir(outside.path().join("child")).unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = format!(
        ".//{}",
        outside.path().display().to_string().trim_start_matches('/')
    );

    let mut config = zone(Some(&root), ".");
    assert!(config.auto_discover_dirs(project.path()).is_empty());
    config.expand_auto_discover(project.path());
    assert!(
        config
            .zones
            .iter()
            .all(|zone| zone.name != "features/child"),
        "{:?}",
        config.zones
    );
}

#[cfg(unix)]
#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn an_auto_discover_link_out_of_the_root_lists_nothing() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir(outside.path().join("child")).unwrap();
    let project = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), project.path().join("features")).unwrap();

    let mut config = zone(None, "features");
    config.expand_auto_discover(project.path());
    assert!(
        config
            .zones
            .iter()
            .all(|zone| zone.name != "features/child"),
        "{:?}",
        config.zones
    );
}
