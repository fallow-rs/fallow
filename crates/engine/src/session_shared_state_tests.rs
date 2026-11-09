//! Optional extraction must remain distinct in every session reuse path.
use super::*;
use fallow_types::extract::SemanticFact;

#[test]
fn shared_state_warm_store_and_disk_disabled_sessions_keep_modes_distinct() {
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("entry.ts"), "window.custom = 1;\n").unwrap();
    let store = Arc::new(WarmParseStore::new(
        crate::warm_parse::WarmParseLimits::default(),
    ));
    let session = |requested, no_cache| {
        AnalysisSession::load_default_with_config(project.path(), |config| {
            config.no_cache = no_cache;
            if requested {
                config.request_shared_state();
            }
        })
        .with_warm_parse(Some(Arc::clone(&store)))
    };
    let has_facts = |modules: &[ModuleInfo]| {
        modules.iter().any(|module| {
            module
                .semantic_facts
                .iter()
                .any(|fact| matches!(fact, SemanticFact::SharedState(_)))
        })
    };
    let standard = session(false, false).parse_modules(false, None);
    assert!(!has_facts(&standard.modules));
    let inventory = session(true, false).parse_modules(false, None);
    assert!(has_facts(&inventory.modules));
    let warm = session(true, false).parse_modules(false, None);
    assert!(Arc::ptr_eq(&inventory.modules, &warm.modules));
    let standard_again = session(false, false).parse_modules(false, None);
    assert!(!has_facts(&standard_again.modules));
    for requested in [false, true] {
        let uncached = session(requested, true);
        let first = uncached.parse_modules(false, None);
        let reused = uncached.parse_modules(false, None);
        assert_eq!(has_facts(&first.modules), requested);
        assert_eq!(has_facts(&reused.modules), requested);
        assert!(Arc::ptr_eq(&first.modules, &reused.modules));
    }
}

#[test]
fn shared_state_embedded_completeness_survives_reuse_and_repairs() {
    for (filename, prefix, suffix) in [
        ("writer.vue", "<script>", "</script>"),
        ("writer.svelte", "<script>", "</script>"),
        ("writer.astro", "---\n", "\n---"),
        ("writer.astro", "<script>", "</script>"),
    ] {
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        std::fs::write(root.join("package.json"), r#"{"main":"entry.ts"}"#).unwrap();
        std::fs::write(
            root.join("entry.ts"),
            format!("import './{filename}'; export const read = () => window.custom;"),
        )
        .unwrap();
        let store = Arc::new(WarmParseStore::new(
            crate::warm_parse::WarmParseLimits::default(),
        ));
        for broken in [false, true, false] {
            let source = if broken {
                "window.custom = 1; const broken = ;"
            } else {
                "window.custom = 1;"
            };
            std::fs::write(root.join(filename), format!("{prefix}{source}{suffix}")).unwrap();
            for (warm, no_cache) in [
                (false, false),
                (false, false),
                (true, false),
                (true, false),
                (true, true),
            ] {
                let session = AnalysisSession::load_default_with_config(root, |config| {
                    config.request_shared_state();
                    config.no_cache = no_cache;
                })
                .with_warm_parse(warm.then(|| Arc::clone(&store)));
                for _ in 0..2 {
                    let result = session.analyze_dead_code().unwrap();
                    let inventory = result.results.shared_state.unwrap();
                    assert_eq!(
                        inventory.complete, !broken,
                        "{filename}, broken={broken}, warm={warm}, no_cache={no_cache}"
                    );
                    if !broken {
                        assert_eq!(inventory.globals[0].slot, "custom");
                        assert_eq!(
                            inventory.globals[0].writes[0].site.path,
                            root.join(filename)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn shared_state_completeness_includes_discovery_and_read_loss() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    std::fs::write(
        root.join("entry.ts"),
        "import './writer'; export const read = () => window.custom;",
    )
    .unwrap();
    std::fs::write(root.join("writer.ts"), "window.custom = 1;").unwrap();
    std::fs::write(
        root.join("large.ts"),
        format!("export const large = '{}';", "x".repeat(2048)),
    )
    .unwrap();
    let session = |limit| {
        AnalysisSession::load_default_with_config(root, |config| {
            config.request_shared_state();
            config.max_file_size_bytes = limit;
        })
    };
    let complete = session(None)
        .analyze_dead_code()
        .unwrap()
        .results
        .shared_state
        .unwrap();
    assert!(complete.complete);
    assert_eq!(complete.globals[0].slot, "custom");
    let limited = session(Some(1024))
        .analyze_dead_code()
        .unwrap()
        .results
        .shared_state
        .unwrap();
    assert!(!limited.complete, "discovery excluded supported source");
    assert_eq!(limited.globals[0].slot, "custom");
    let unreadable = session(None);
    std::fs::remove_file(root.join("writer.ts")).unwrap();
    let inventory = unreadable
        .analyze_dead_code()
        .unwrap()
        .results
        .shared_state
        .unwrap();
    assert!(!inventory.complete, "a discovered source could not be read");
}
