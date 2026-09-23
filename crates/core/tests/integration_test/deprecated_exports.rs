use super::common::{create_config, fixture_path};

fn analyze_with_rule(severity: fallow_config::Severity) -> fallow_core::results::AnalysisResults {
    let mut config = create_config(fixture_path("deprecated-export-in-use"));
    config.rules.deprecated_exports_in_use = severity;
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn finding<'a>(
    results: &'a fallow_core::results::AnalysisResults,
    name: &str,
) -> Option<&'a fallow_types::results::DeprecatedExportInUse> {
    results
        .deprecated_exports_in_use
        .iter()
        .map(|finding| &finding.export)
        .find(|export| export.export_name == name)
}

fn reported_names(results: &fallow_core::results::AnalysisResults) -> Vec<&str> {
    results
        .deprecated_exports_in_use
        .iter()
        .map(|finding| finding.export.export_name.as_str())
        .collect()
}

#[test]
fn rule_defaults_to_off() {
    let config = create_config(fixture_path("deprecated-export-in-use"));
    assert_eq!(
        config.rules.deprecated_exports_in_use,
        fallow_config::Severity::Off
    );
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.deprecated_exports_in_use.is_empty(),
        "an off rule reports nothing: {:?}",
        reported_names(&results)
    );
}

#[test]
fn deprecated_export_with_importers_reports_count_and_capped_sorted_sample() {
    let results = analyze_with_rule(fallow_config::Severity::Warn);
    let old = finding(&results, "oldHelper").expect("oldHelper is reported");

    assert_eq!(old.consumer_count, 12, "the count is exact");
    assert_eq!(
        old.consumers.len(),
        fallow_types::results::DEPRECATED_CONSUMER_SAMPLE_CAP,
        "the sample is capped"
    );
    let paths: Vec<String> = old
        .consumers
        .iter()
        .map(|c| c.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted, "the sample is sorted by path");
    assert_eq!(paths.first().map(String::as_str), Some("c01.ts"));
    assert!(old.consumers.iter().all(
        |c| c.line == 1 && c.kind == fallow_types::results::DeprecatedConsumerKind::NamedImport
    ));
    assert_eq!(
        old.deprecated_reason.as_deref(),
        Some("Use newHelper instead. It goes away in the next major."),
        "the message is plain text, stops at the next tag and joins lines"
    );
    assert!(!old.public_api);
}

#[test]
fn namespace_consumer_is_reported_with_its_kind() {
    let results = analyze_with_rule(fallow_config::Severity::Warn);
    let ns = finding(&results, "nsOld").expect("nsOld is reported");
    assert_eq!(ns.consumer_count, 1);
    assert_eq!(
        ns.consumers[0].kind,
        fallow_types::results::DeprecatedConsumerKind::NamespaceImport
    );

    let whole = finding(&results, "barrelOnlyOld").expect("barrelOnlyOld is reported");
    assert_eq!(
        whole.consumers[0].kind,
        fallow_types::results::DeprecatedConsumerKind::NamespaceImport,
        "a namespace import of a barrel credits the declaring export"
    );
}

#[test]
fn deprecated_and_unused_export_is_one_decorated_unused_export() {
    let results = analyze_with_rule(fallow_config::Severity::Warn);
    assert!(finding(&results, "deadOld").is_none());
    let dead = results
        .unused_exports
        .iter()
        .find(|finding| finding.export.export_name == "deadOld")
        .expect("deadOld is an unused export");
    assert!(dead.export.deprecated);
    assert_eq!(dead.export.deprecated_reason.as_deref(), Some("gone soon"));

    let bare = results
        .unused_exports
        .iter()
        .find(|finding| {
            finding.export.export_name == "viaBarrel"
                && finding.export.path.ends_with("barrel-only.ts")
        })
        .expect("viaBarrel is an unused export");
    assert!(bare.export.deprecated);
    assert_eq!(
        bare.export.deprecated_reason, None,
        "a bare tag has no reason"
    );

    for reported in &results.deprecated_exports_in_use {
        let export = &reported.export;
        let unused_twice = results
            .unused_exports
            .iter()
            .map(|f| &f.export)
            .chain(results.unused_types.iter().map(|f| &f.export))
            .any(|unused| unused.path == export.path && unused.export_name == export.export_name);
        assert!(
            !unused_twice,
            "{} is reported only once",
            export.export_name
        );
    }
}

#[test]
fn undecorated_unused_export_serializes_without_deprecation_keys() {
    let results = analyze_with_rule(fallow_config::Severity::Warn);
    let plain = results
        .unused_exports
        .iter()
        .find(|finding| finding.export.export_name == "newHelper")
        .expect("newHelper is an unused export");
    let json = serde_json::to_value(plain).expect("serialize");
    assert!(
        json.get("deprecated").is_none(),
        "never `deprecated: false`"
    );
    assert!(json.get("deprecated_reason").is_none());
}

#[test]
fn entry_point_exports_are_public_api_without_removal_claim() {
    let results = analyze_with_rule(fallow_config::Severity::Warn);

    let entry = finding(&results, "entryOld").expect("entryOld has an internal importer");
    assert!(entry.public_api);
    assert_eq!(entry.consumer_count, 1);

    assert!(
        finding(&results, "entryUnusedOld").is_none(),
        "an entry export without internal consumers is not reported"
    );
    assert!(
        !results
            .unused_exports
            .iter()
            .any(|f| f.export.export_name == "entryUnusedOld"),
        "and it is not an unused export either"
    );

    let public = finding(&results, "publicOld").expect("publicOld is re-exported by the entry");
    assert!(
        public.public_api,
        "a re-export chain that reaches an entry point is public API, not removable"
    );
    assert_eq!(
        public.consumers[0].kind,
        fallow_types::results::DeprecatedConsumerKind::ReExport
    );
    assert!(public.consumers[0].path.ends_with("index.ts"));

    let deep = finding(&results, "deepOld").expect("deepOld reaches the entry by two star hops");
    assert!(deep.public_api);
    assert!(
        deep.consumers[0].path.ends_with("mid-barrel.ts"),
        "a synthesized re-export reference points at the barrel statement"
    );
}

#[test]
fn tag_guards_and_suppression_comment_report_nothing() {
    let results = analyze_with_rule(fallow_config::Severity::Warn);
    let names = reported_names(&results);
    for name in ["guardedFoo", "guardedString", "suppressedOld"] {
        assert!(
            !names.contains(&name),
            "{name} must not be reported: {names:?}"
        );
    }
}

#[test]
fn consumers_in_unreachable_files_do_not_count() {
    let results = analyze_with_rule(fallow_config::Severity::Warn);
    assert!(
        finding(&results, "onlyDeadUse").is_none(),
        "a deprecated export used only by an unreachable file is not in use"
    );
    let old = finding(&results, "oldHelper").expect("oldHelper is reported");
    assert_eq!(
        old.consumer_count, 12,
        "the unreachable importer does not count"
    );
    assert!(
        old.consumers
            .iter()
            .all(|consumer| !consumer.path.ends_with("dead.ts"))
    );
}

#[test]
fn consumer_sample_is_the_capped_prefix_of_the_count() {
    let results = analyze_with_rule(fallow_config::Severity::Warn);
    for finding in &results.deprecated_exports_in_use {
        let export = &finding.export;
        assert_eq!(
            export.consumers.len(),
            export
                .consumer_count
                .min(fallow_types::results::DEPRECATED_CONSUMER_SAMPLE_CAP),
            "{}",
            export.export_name
        );
    }
}

#[test]
fn star_re_export_does_not_carry_a_default_export_to_the_entry() {
    let results = analyze_with_rule(fallow_config::Severity::Warn);
    let legacy = finding(&results, "default").expect("the deprecated default export is used");
    assert!(
        !legacy.public_api,
        "`export *` never forwards `default`, so the entry does not expose it"
    );
}

#[test]
fn tags_do_not_leak_to_later_statements_without_semicolons() {
    let results = analyze_with_rule(fallow_config::Severity::Warn);
    let first = finding(&results, "firstNoSemi").expect("firstNoSemi is reported");
    assert_eq!(first.deprecated_reason.as_deref(), Some("use lastNoSemi"));
    assert!(finding(&results, "secondNoSemi").is_none());

    let unused: Vec<&str> = results
        .unused_exports
        .iter()
        .filter(|f| f.export.path.ends_with("nosemi.ts"))
        .map(|f| f.export.export_name.as_str())
        .collect();
    assert_eq!(
        unused,
        ["afterInternal"],
        "`@internal` covers only its own statement, so the next export is unused"
    );
}
