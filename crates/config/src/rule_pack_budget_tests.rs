//! Bounded work for repeated rule pack paths and repeated zone references.

use std::time::{Duration, Instant};

use super::*;

/// The wall-clock bound of one bounded load in a debug build.
const LOAD_BOUND: Duration = Duration::from_secs(3);

fn pack_with_rules(rules: usize) -> String {
    let rules: Vec<serde_json::Value> = (0..rules)
        .map(|index| {
            serde_json::json!({ "id": format!("r{index}"), "kind": "banned-call", "callees": ["eval"] })
        })
        .collect();
    serde_json::json!({ "version": 1, "name": "p", "rules": rules }).to_string()
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn a_rule_pack_listed_many_times_is_read_once() {
    const REPEATS: usize = 2_000;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.json"), pack_with_rules(200)).unwrap();
    let mut paths = vec!["p.json".to_owned(); REPEATS];
    paths.push("./p.json".to_owned());

    let start = Instant::now();
    let errors = load_rule_packs(dir.path(), &paths).expect_err("repeated pack path");
    let elapsed = start.elapsed();

    assert_eq!(errors.len(), 1, "one error for the repeated path");
    assert!(
        errors[0].message.contains("listed more than once"),
        "{}",
        errors[0].message
    );
    assert!(elapsed < LOAD_BOUND, "{REPEATS} entries took {elapsed:?}");
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn distinct_rule_packs_still_load() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.json"), pack_with_rules(1)).unwrap();
    std::fs::write(
        dir.path().join("b.json"),
        pack_with_rules(1).replace(r#""name":"p""#, r#""name":"q""#),
    )
    .unwrap();

    let packs = load_rule_packs(dir.path(), &["a.json".to_owned(), "b.json".to_owned()])
        .expect("distinct packs load");

    assert_eq!(packs.len(), 2);
}

fn boundaries_with_zone(name: &str) -> BoundaryConfig {
    BoundaryConfig {
        zones: vec![crate::config::BoundaryZone {
            name: name.to_owned(),
            patterns: vec!["src/**".to_owned()],
            auto_discover: Vec::new(),
            root: None,
            public_files: vec![],
        }],
        ..BoundaryConfig::default()
    }
}

fn zone_errors(zones: &[String], configured_zone: &str) -> Vec<RulePackError> {
    let dir = tempfile::tempdir().unwrap();
    let pack = serde_json::json!({ "version": 1, "name": "p", "rules": [
        { "id": "r", "kind": "banned-call", "callees": ["eval"], "zones": zones }
    ] });
    std::fs::write(dir.path().join("p.json"), pack.to_string()).unwrap();
    let paths = ["p.json".to_owned()];
    let packs = load_rule_packs(dir.path(), &paths).unwrap();
    validate_rule_pack_zones(
        dir.path(),
        &boundaries_with_zone(configured_zone),
        &paths,
        &packs,
    )
}

fn message_bytes(errors: &[RulePackError]) -> usize {
    errors.iter().map(|error| error.message.len()).sum()
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn repeated_unknown_zone_gives_one_short_error() {
    let long_zone = "z".repeat(10_000);
    let zones = vec!["b".to_owned(); 2_000];

    let errors = zone_errors(&zones, &long_zone);

    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("unknown zone 'b'"));
    assert!(
        message_bytes(&errors) < 4_096,
        "{} bytes",
        message_bytes(&errors)
    );
}

#[test]
#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
fn many_distinct_unknown_zones_give_a_bounded_error_list() {
    let zones: Vec<String> = (0..2_000).map(|index| format!("u{index}")).collect();

    let errors = zone_errors(&zones, "domain");

    assert!(
        errors.len() <= MAX_UNKNOWN_ZONE_ERRORS + 1,
        "{}",
        errors.len()
    );
    let last = errors.last().unwrap();
    assert!(
        last.message.contains("more unknown zones"),
        "{}",
        last.message
    );
    assert!(errors[0].message.contains("configured zones: domain"));
}
