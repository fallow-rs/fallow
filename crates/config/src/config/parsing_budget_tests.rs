//! Bounded work for repeated `extends` entries.

use std::time::{Duration, Instant};

use super::*;

/// The wall-clock bound of one bounded load in a debug build.
const LOAD_BOUND: Duration = Duration::from_secs(3);

/// A config object with `chains` keys, each nested `depth` levels deep.
fn nested_base(chains: usize, depth: usize) -> serde_json::Value {
    let mut base = serde_json::Map::new();
    for chain in 0..chains {
        let mut value = serde_json::json!(0);
        for _ in 0..depth {
            value = serde_json::json!({ "a": value });
        }
        base.insert(format!("k{chain}"), value);
    }
    serde_json::json!({ "rules": { "x": serde_json::Value::Object(base) } })
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn repeated_extends_entries_merge_the_base_once() {
    const REPEATS: usize = 5_000;
    let dir = tempfile::tempdir().unwrap();
    let base = nested_base(100, 50);
    std::fs::write(dir.path().join("b.json"), base.to_string()).unwrap();
    let mut extends = vec!["./b.json".to_owned(); REPEATS];
    extends.push("b.json".to_owned());
    let root = dir.path().join(".fallowrc.json");
    std::fs::write(&root, serde_json::json!({ "extends": extends }).to_string()).unwrap();

    let start = Instant::now();
    let merged = resolve_extends(&root, &mut FxHashSet::default(), 0).expect("extends resolve");
    let elapsed = start.elapsed();

    assert_eq!(merged, base);
    assert!(
        elapsed < LOAD_BOUND,
        "{REPEATS} entries that name one base took {elapsed:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn repeated_extends_entries_keep_the_merge_result_of_the_last_entry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.json"), r#"{"k": "a", "only_a": 1}"#).unwrap();
    std::fs::write(
        dir.path().join("b.json"),
        r#"{"k": "b", "nested": {"x": 1}}"#,
    )
    .unwrap();
    let root = dir.path().join(".fallowrc.json");
    std::fs::write(
        &root,
        r#"{"extends": ["./a.json", "./b.json", "a.json"], "own": true}"#,
    )
    .unwrap();

    let merged = resolve_extends(&root, &mut FxHashSet::default(), 0).expect("extends resolve");

    assert_eq!(
        merged,
        serde_json::json!({"k": "a", "only_a": 1, "nested": {"x": 1}, "own": true})
    );
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn too_many_distinct_extends_entries_are_a_config_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut extends = Vec::new();
    for index in 0..=MAX_EXTENDS_ENTRIES {
        let name = format!("b{index}.json");
        std::fs::write(dir.path().join(&name), "{}").unwrap();
        extends.push(name);
    }
    let root = dir.path().join(".fallowrc.json");
    std::fs::write(&root, serde_json::json!({ "extends": extends }).to_string()).unwrap();

    let error = resolve_extends(&root, &mut FxHashSet::default(), 0)
        .expect_err("too many extends entries")
        .to_string();

    assert!(error.contains("extends"), "{error}");
    assert!(error.contains(&MAX_EXTENDS_ENTRIES.to_string()), "{error}");
}
