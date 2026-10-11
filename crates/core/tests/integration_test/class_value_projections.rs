use super::common::{create_config, fixture_path};

fn assert_indexed_member(fixture: &str, used: bool) {
    let mut config = create_config(fixture_path(fixture));
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.unresolved_imports.is_empty(),
        "{fixture}: unresolved imports"
    );
    let unused: Vec<_> = results
        .unused_class_members
        .iter()
        .map(|finding| {
            format!(
                "{}.{}",
                finding.member.parent_name, finding.member.member_name
            )
        })
        .collect();
    if fixture == "class-value-indexed-imported-owner" {
        for member in ["Decoy.go", "Decoy.dead"] {
            assert!(
                unused.iter().any(|name| name == member),
                "{fixture}: wrong owner credited: {unused:?}"
            );
        }
    }
    for member in ["C.dead", "C.unknownOnly", "C.goStatic", "C.mixinOnly"] {
        assert!(
            unused.iter().any(|name| name == member),
            "{fixture}: {member} must remain unused: {unused:?}"
        );
    }
    assert!(
        !unused.iter().any(|name| name == "C.direct"),
        "{fixture}: direct control: {unused:?}"
    );
    assert_eq!(
        !unused.iter().any(|name| name == "C.go"),
        used,
        "{fixture}: indexed receiver: {unused:?}"
    );
}

#[test]
fn indexed_array_element_property_credits_only_proven_class_member() {
    assert_indexed_member("class-value-indexed-array-param", true);
}

#[test]
fn indexed_numeric_parameter_credits_proven_homogeneous_element() {
    assert_indexed_member("class-value-indexed-dynamic-number", true);
}

#[test]
fn indexed_named_readonly_and_direct_class_elements() {
    for fixture in [
        "class-value-indexed-named-element",
        "class-value-indexed-readonly-inline",
        "class-value-indexed-direct-class",
    ] {
        assert_indexed_member(fixture, true);
    }
}

#[test]
fn indexed_unknown_tuple_and_shadow_roots_do_not_borrow_class() {
    for fixture in [
        "class-value-indexed-unknown-element",
        "class-value-indexed-tuple-distinct",
        "class-value-indexed-shadowed-array",
    ] {
        assert_indexed_member(fixture, false);
    }
    assert_indexed_member("class-value-direct-property-control", true);
}

#[test]
fn indexed_imported_element_uses_its_declaring_modules_property_type() {
    assert_indexed_member("class-value-indexed-imported-owner", true);
}

fn assert_review_projection(fixture: &str, used: bool, custom_container: bool) {
    let mut config = create_config(fixture_path(fixture));
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(results.unresolved_imports.is_empty(), "{fixture}");
    let mut actual: Vec<_> = results
        .unused_class_members
        .iter()
        .filter(|finding| matches!(finding.member.parent_name.as_str(), "C" | "Other"))
        .map(|finding| {
            format!(
                "{}.{}",
                finding.member.parent_name, finding.member.member_name
            )
        })
        .filter(|name| !custom_container || name != "Other.used")
        .collect();
    actual.sort();
    let mut expected = vec!["C.dead", "Other.dead"];
    if !used {
        expected.push("C.used");
    }
    if !custom_container {
        expected.push("Other.used");
    }
    expected.sort_unstable();
    assert_eq!(actual, expected, "{fixture}");
}

const INDEXED_REVIEW_FIXTURES: &[(&str, bool, bool)] = &[
    ("class-value-review-shadow-array-capture-off", false, true),
    ("class-value-review-shadow-array-capture-on", false, true),
    (
        "class-value-review-shadow-readonly-array-capture-off",
        false,
        true,
    ),
    (
        "class-value-review-shadow-readonly-array-capture-on",
        false,
        true,
    ),
    (
        "class-value-review-builtin-array-control-capture-off",
        true,
        false,
    ),
    (
        "class-value-review-builtin-array-control-capture-on",
        true,
        false,
    ),
    (
        "class-value-review-builtin-readonly-array-control-capture-off",
        true,
        false,
    ),
    (
        "class-value-review-builtin-readonly-array-control-capture-on",
        true,
        false,
    ),
    (
        "class-value-review-typed-alias-write-capture-off",
        false,
        false,
    ),
    (
        "class-value-review-typed-alias-write-capture-on",
        false,
        false,
    ),
    (
        "class-value-review-ordinary-alias-write-control-capture-off",
        false,
        false,
    ),
    (
        "class-value-review-ordinary-alias-write-control-capture-on",
        false,
        false,
    ),
    (
        "class-value-review-typed-alias-untouched-control-capture-off",
        true,
        false,
    ),
    (
        "class-value-review-typed-alias-untouched-control-capture-on",
        true,
        false,
    ),
    (
        "class-value-review-ordinary-alias-untouched-control-capture-off",
        true,
        false,
    ),
    (
        "class-value-review-ordinary-alias-untouched-control-capture-on",
        true,
        false,
    ),
    (
        "class-value-review-conditional-alternate-read-capture-off",
        true,
        false,
    ),
    (
        "class-value-review-conditional-alternate-read-capture-on",
        true,
        false,
    ),
    (
        "class-value-review-conditional-untouched-control-capture-off",
        true,
        false,
    ),
    (
        "class-value-review-conditional-untouched-control-capture-on",
        true,
        false,
    ),
];

#[test]
fn indexed_review_builtin_alias_and_branch_callers() {
    for &(fixture, used, custom_container) in INDEXED_REVIEW_FIXTURES {
        assert_review_projection(fixture, used, custom_container);
    }
}

#[test]
fn indexed_conditional_mutation_does_not_restore_postjoin_proof() {
    for fixture in [
        "class-value-review-conditional-postjoin-capture-off",
        "class-value-review-conditional-postjoin-capture-on",
    ] {
        assert_review_projection(fixture, false, false);
    }
}

#[test]
fn indexed_shared_subclass_readers_preserve_container_and_branch_authority() {
    assert_review_projection("class-value-review-subclass-array-shadow", false, true);
    assert_review_projection(
        "class-value-review-subclass-array-builtin-control",
        true,
        false,
    );
    assert_review_projection(
        "class-value-review-subclass-readonly-array-shadow",
        false,
        true,
    );
    assert_review_projection(
        "class-value-review-subclass-readonly-array-builtin-control",
        true,
        false,
    );
    assert_review_projection("class-value-review-subclass-overlay-write", false, false);
    assert_review_projection(
        "class-value-review-subclass-overlay-no-write-control",
        true,
        false,
    );
}

const REFUSAL_ARENA_BUDGET: usize = 8192;
const REFUSAL_SHAPE_WIDTH: usize = 128;

fn refusal_arena_pressure() -> String {
    use std::fmt::Write as _;
    (0..REFUSAL_ARENA_BUDGET).fold(String::new(), |mut source, index| {
        writeln!(source, "const filler{index}: number = 0;").expect("refusal fixture setup");
        source
    })
}

fn refusal_work_pressure() -> String {
    "if (flag) { pad.slot0 = opaque(); }\n".repeat(REFUSAL_ARENA_BUDGET / REFUSAL_SHAPE_WIDTH + 2)
}

fn refusal_source(case: &str, subclass: bool) -> String {
    use std::fmt::Write as _;
    let target = if subclass { "Sub" } else { "C" };
    let heritage = if subclass {
        "class Sub extends C {}"
    } else {
        ""
    };
    let fields = (0..REFUSAL_SHAPE_WIDTH).fold(String::new(), |mut source, index| {
        write!(source, "slot{index}: Other;").expect("refusal fixture setup");
        source
    });
    let values = (0..REFUSAL_SHAPE_WIDTH).fold(String::new(), |mut source, index| {
        write!(source, "slot{index}: new Other(),").expect("refusal fixture setup");
        source
    });
    let pressure = if case.starts_with("below") {
        String::new()
    } else if case.starts_with("arena") {
        refusal_arena_pressure()
    } else {
        refusal_work_pressure()
    };
    let body = match case {
        "arena-branch" | "below-branch" => {
            format!("{pressure} if(flag) {{ saved.widget = opaque(); saved.widget.after(); }}")
        }
        "work-write" => format!("{pressure} items[0].widget = opaque(); saved.widget.after();"),
        "work-join" | "below-join" => {
            format!("if(flag) {{ saved.widget = opaque(); {pressure} }} saved.widget.after();")
        }
        "work-alternate" => format!(
            "if(flag) {{ saved.widget = opaque(); {pressure} }} else {{ saved.widget.alternate(); }}"
        ),
        "arena-no-write" => format!("{pressure} saved.widget.after();"),
        "arena-alternate" => format!(
            "{pressure} if(flag) {{ saved.widget = opaque(); }} else {{ saved.widget.alternate(); }}"
        ),
        "arena-independent" => format!(
            "{pressure} if(flag) {{ saved.widget = opaque(); independent.widget = opaque(); saved.widget = new {target}(); independent.widget.after(); }}"
        ),
        _ => unreachable!("declared refusal case"),
    };
    format!("import {{C,Other}} from './classes'; {heritage}
        function opaque() {{ return {{after() {{}}, alternate() {{}}}}; }}
        function consume(items: {{ widget: {target} }}[], otherItems: {{ widget: {target} }}[], untouchedItems: {{ widget: {target} }}[], padding: {{{fields}}}[], flag: boolean) {{
            const saved = items[0]; const scalar = saved.widget; const pad = padding[0]; const independent = otherItems[0]; const untouched = untouchedItems[0];
            saved.widget.before(); {body} scalar.snapshot(); untouched.widget.untouched();
        }}
        consume([{{widget: new {target}()}}], [{{widget: new {target}()}}], [{{widget: new {target}()}}], [{{{values}}}], true);
        new C().direct();")
}

fn analyze_refusal_source(source: &str) -> Vec<String> {
    let directory = tempfile::tempdir().expect("refusal fixture setup");
    std::fs::write(
        directory.path().join("package.json"),
        r#"{"name":"refusal-case","main":"index.ts"}"#,
    )
    .expect("refusal fixture setup");
    std::fs::write(directory.path().join("index.ts"), source).expect("refusal fixture setup");
    std::fs::write(directory.path().join("classes.ts"),
        "export class C { before() {} after() {} alternate() {} snapshot() {} direct() {} dead() {} untouched() {} } export class Other {}"
    ).expect("refusal fixture setup");
    let mut config = create_config(directory.path().to_path_buf());
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("refusal fixture analysis");
    assert!(
        results.unresolved_imports.is_empty(),
        "refusal imports must resolve"
    );
    let mut unused: Vec<_> = results
        .unused_class_members
        .iter()
        .filter(|finding| finding.member.parent_name == "C")
        .map(|finding| finding.member.member_name.clone())
        .collect();
    unused.sort();
    unused
}

#[test]
fn indexed_refusal_invalidates_later_reads_without_erasing_snapshots() {
    let mut failures = Vec::new();
    for subclass in [false, true] {
        for case in [
            "arena-branch",
            "work-write",
            "work-join",
            "work-alternate",
            "arena-no-write",
            "arena-alternate",
            "arena-independent",
            "below-branch",
            "below-join",
        ] {
            let source = refusal_source(case, subclass);
            let actual = analyze_refusal_source(&source);
            let mut expected = vec!["dead"];
            if case != "arena-no-write" {
                expected.push("after");
            }
            if !matches!(case, "work-alternate" | "arena-alternate") {
                expected.push("alternate");
            }
            expected.sort_unstable();
            if actual != expected {
                failures.push(format!(
                    "{case}, subclass={subclass}: {actual:?} != {expected:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn indexed_refused_container_attachment_retains_builtin_authority() {
    let pressure = refusal_arena_pressure();
    let mut failures = Vec::new();
    for container in ["Array", "ReadonlyArray"] {
        for shadow in [false, true] {
            let alias = if shadow {
                format!("type {container}<T> = Other[];")
            } else {
                String::new()
            };
            let source = format!("import {{C,Other}} from './classes'; class Sub extends C {{}} {alias} {pressure}
                function consume(items: {container}<Sub>) {{ for(const saved of items) {{ saved.after(); }} }}
                consume([]); new C().direct();");
            let actual = analyze_refusal_source(&source);
            let expected = if shadow {
                vec![
                    "after",
                    "alternate",
                    "before",
                    "dead",
                    "snapshot",
                    "untouched",
                ]
            } else {
                vec!["alternate", "before", "dead", "snapshot", "untouched"]
            };
            if actual != expected {
                failures.push(format!(
                    "{container}, shadow={shadow}: {actual:?} != {expected:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn indexed_carried_container_observations_survive_enrichment_refusal() {
    let mut failures = Vec::new();
    for subclass in [false, true] {
        let target = if subclass { "Sub" } else { "C" };
        for container in ["Array", "ReadonlyArray"] {
            for case in [
                "arena-branch",
                "work-write",
                "work-alternate",
                "below-branch",
            ] {
                let source = refusal_source(case, subclass).replacen(
                    &format!("items: {{ widget: {target} }}[]"),
                    &format!("items: {container}<{{ widget: {target} }}>"),
                    1,
                );
                let actual = analyze_refusal_source(&source);
                let expected = if case == "work-alternate" {
                    vec!["after", "dead"]
                } else {
                    vec!["after", "alternate", "dead"]
                };
                if actual != expected {
                    failures.push(format!(
                        "{container}, {case}, subclass={subclass}: {actual:?} != {expected:?}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
