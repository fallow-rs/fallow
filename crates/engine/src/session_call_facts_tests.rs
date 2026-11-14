//! A session gives call facts exactly when its config reads them, in each
//! reuse path: the persisted cache, the warm store and a cache-free session.
use super::*;

#[test]
fn call_facts_follow_the_config_in_every_session_reuse_path() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join("entry.ts"),
        "export function format(value, compact) { return compact ? value : value; }\n\
         format(1, true);\n\
         format(2, true);\n",
    )
    .unwrap();
    let store = Arc::new(WarmParseStore::new(
        crate::warm_parse::WarmParseLimits::default(),
    ));
    let session = |reads_call_facts: bool, no_cache: bool| {
        AnalysisSession::load_default_with_config(project.path(), |config| {
            config.no_cache = no_cache;
            if reads_call_facts {
                config.rules.constant_parameters = fallow_config::Severity::Warn;
            }
        })
        .with_warm_parse(Some(Arc::clone(&store)))
    };
    let has_facts = |modules: &[ModuleInfo]| {
        modules
            .iter()
            .any(|module| module.function_contracts.is_some())
    };
    let first = session(false, false).parse_modules(false, None);
    assert!(!has_facts(&first.modules));
    for (reads_call_facts, no_cache) in [
        (true, false),
        (false, false),
        (true, false),
        (false, true),
        (true, true),
    ] {
        let parsed = session(reads_call_facts, no_cache).parse_modules(false, None);
        if reads_call_facts {
            assert!(has_facts(&parsed.modules), "no_cache={no_cache}");
        } else if no_cache {
            assert!(!has_facts(&parsed.modules));
        }
    }

    // A session without the warm store reads the persisted cache, which now
    // holds the facts. A session that reads no call facts does not decode them.
    let disk_only = |reads_call_facts: bool| {
        AnalysisSession::load_default_with_config(project.path(), |config| {
            if reads_call_facts {
                config.rules.constant_parameters = fallow_config::Severity::Warn;
            }
        })
        .parse_modules(false, None)
    };
    let without = disk_only(false);
    assert_eq!(without.metrics.cache_misses, 0);
    assert!(!has_facts(&without.modules));
    let with = disk_only(true);
    assert_eq!(with.metrics.cache_misses, 0);
    assert!(has_facts(&with.modules));
}
