//! Bounded work for plugin files that the config names many times.

use std::time::{Duration, Instant};

use super::*;

/// The wall-clock bound of one bounded discovery in a debug build.
const DISCOVER_BOUND: Duration = Duration::from_secs(3);

/// A valid plugin with `enablers` package names, about 12 bytes each.
fn plugin_json(name: &str, enablers: usize) -> String {
    let enablers: Vec<String> = (0..enablers).map(|index| format!("pkg-{index}")).collect();
    serde_json::json!({ "name": name, "enablers": enablers }).to_string()
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn a_plugin_file_named_many_times_is_read_once() {
    const REPEATS: usize = 2_000;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("d")).unwrap();
    std::fs::write(dir.path().join("d/p.json"), plugin_json("p", 20_000)).unwrap();
    let mut paths = vec!["d/p.json".to_owned(); REPEATS];
    paths.extend(vec!["d".to_owned(); REPEATS]);
    paths.push("./d/../d/p.json".to_owned());

    let start = Instant::now();
    let plugins = discover_external_plugins(dir.path(), &paths);
    let elapsed = start.elapsed();

    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].name, "p");
    assert!(elapsed < DISCOVER_BOUND, "discovery took {elapsed:?}");
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn plugin_files_above_the_limit_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("d")).unwrap();
    for index in 0..=MAX_EXTERNAL_PLUGIN_FILES {
        std::fs::write(
            dir.path().join(format!("d/p{index:04}.json")),
            plugin_json(&format!("p{index:04}"), 1),
        )
        .unwrap();
    }

    let plugins = discover_external_plugins(dir.path(), &["d".to_owned()]);

    assert_eq!(plugins.len(), MAX_EXTERNAL_PLUGIN_FILES);
}
