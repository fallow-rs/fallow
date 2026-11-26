#![allow(
    clippy::unwrap_used,
    reason = "tests use unwrap to keep fixture setup concise"
)]

use super::tests::make_test_snapshot;
use super::{VitalSignsSnapshot, load_trend_baseline, save_snapshot};

#[test]
fn save_snapshot_keeps_two_implicit_snapshots_of_one_second() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let first = make_test_snapshot("2026-01-01T00:00:00Z", Some(80.0));
    let second = make_test_snapshot("2026-01-01T00:00:00Z", Some(60.0));

    let first_path = save_snapshot(&first, root, None).unwrap();
    let second_path = save_snapshot(&second, root, None).unwrap();

    assert_ne!(first_path, second_path);
    let first_saved: VitalSignsSnapshot =
        serde_json::from_str(&std::fs::read_to_string(&first_path).unwrap()).unwrap();
    let second_saved: VitalSignsSnapshot =
        serde_json::from_str(&std::fs::read_to_string(&second_path).unwrap()).unwrap();
    assert_eq!(first_saved.score, Some(80.0));
    assert_eq!(second_saved.score, Some(60.0));
}

#[test]
fn trend_baseline_is_the_last_save_of_one_second() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for score in [
        80.0, 70.0, 75.0, 60.0, 65.0, 50.0, 55.0, 40.0, 45.0, 30.0, 20.0,
    ] {
        save_snapshot(
            &make_test_snapshot("2026-01-01T00:00:00Z", Some(score)),
            root,
            None,
        )
        .unwrap();
    }

    let baseline = load_trend_baseline(root, None).unwrap().unwrap();

    assert_eq!(baseline.snapshot.score, Some(20.0));
    assert!(baseline.path.ends_with("2026-01-01T00-00-00Z-11.json"));
}

/// A snapshot directory link out of the project is not listed, and a
/// snapshot file link out of the project is not read.
#[test]
#[cfg(unix)]
fn trend_baseline_skips_snapshot_links_out_of_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    let outside = dir.path().join("elsewhere");
    std::fs::create_dir_all(root.join(".fallow")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let outside_snapshot = outside.join("2026-01-01T00-00-00Z.json");
    let snapshot = make_test_snapshot("2026-01-01T00:00:00Z", Some(42.0));
    std::fs::write(&outside_snapshot, serde_json::to_string(&snapshot).unwrap()).unwrap();

    std::os::unix::fs::symlink(&outside, root.join(".fallow/snapshots")).unwrap();
    assert!(load_trend_baseline(&root, None).unwrap().is_none());

    std::fs::remove_file(root.join(".fallow/snapshots")).unwrap();
    std::fs::create_dir(root.join(".fallow/snapshots")).unwrap();
    std::os::unix::fs::symlink(
        &outside_snapshot,
        root.join(".fallow/snapshots/2026-01-01T00-00-00Z.json"),
    )
    .unwrap();
    assert!(load_trend_baseline(&root, None).unwrap().is_none());
}
