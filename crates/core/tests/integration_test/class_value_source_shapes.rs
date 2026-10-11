use super::common::create_config;

#[test]
fn indexed_source_shapes_preserve_complete_member_projection() {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/class-value-source-shapes/cases.json"
    ))
    .expect("source-shape fixtures");
    let mut failures = Vec::new();
    for case in fixtures["cases"].as_array().expect("case list") {
        let directory = tempfile::tempdir().expect("fixture directory");
        std::fs::write(directory.path().join("package.json"),
            r#"{"name":"indexed-source-shape","type":"module","main":"index.ts","dependencies":{"vue":"*","@vue/reactivity":"*","foreign":"*"}}"#)
            .expect("fixture package");
        for (name, content) in case["sources"].as_object().expect("case sources") {
            let path = directory.path().join(name);
            std::fs::create_dir_all(path.parent().expect("source parent"))
                .expect("source directory");
            std::fs::write(path, content.as_str().expect("source text")).expect("fixture source");
        }
        let mut config = create_config(directory.path().to_path_buf());
        config.rules.unused_class_members = fallow_config::Severity::Error;
        let results = fallow_core::analyze(&config).expect("source-shape analysis");
        assert!(
            results.unresolved_imports.is_empty(),
            "{}: unresolved imports",
            case["id"]
        );
        let mut actual: Vec<_> = results
            .unused_class_members
            .iter()
            .filter(|finding| finding.member.parent_name == "C")
            .map(|finding| format!("C.{}", finding.member.member_name))
            .collect();
        actual.sort();
        let expected: Vec<_> = case["unused"]
            .as_array()
            .expect("unused projection")
            .iter()
            .map(|member| member.as_str().expect("member name"))
            .collect();
        if actual != expected {
            failures.push(format!("{}: {actual:?} != {expected:?}", case["id"]));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn indexed_source_shapes_keep_ordinary_and_optional_capture_in_agreement() {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/class-value-source-shapes/cases.json"
    ))
    .expect("source-shape fixtures");
    let mut failures = Vec::new();
    for case in fixtures["cases"].as_array().expect("case list") {
        for capture in [false, true] {
            let source = case["sources"]["index.ts"]
                .as_str()
                .expect("TypeScript source");
            let source = if capture {
                format!(
                    "class CaptureBase {{}} class CaptureEnabled extends CaptureBase {{}}\n{source}"
                )
            } else {
                source.to_string()
            };
            let info = fallow_extract::parse_from_content(
                fallow_types::discover::FileId(0),
                std::path::Path::new("index.ts"),
                &source,
            );
            for member in ["used", "dead"] {
                let expected = case["used"]
                    .as_array()
                    .expect("used projection")
                    .iter()
                    .any(|value| value.as_str() == Some(&format!("C.{member}")));
                let actual = info
                    .member_accesses
                    .iter()
                    .any(|access| access.object == "C" && access.member == member);
                if actual != expected {
                    failures.push(format!(
                        "{}, capture={capture}, {member}: {actual} != {expected}",
                        case["id"]
                    ));
                }
            }
            assert!(
                !info.whole_object_uses.iter().any(|name| name == "C"),
                "{}",
                case["id"]
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn subclass_refusal_source(pressure_kind: Option<bool>, nested: bool, root: bool) -> String {
    use std::fmt::Write;
    const CONSTRUCTOR_WORK_BUDGET: usize = 4096;
    const VALUE_ARENA_BUDGET: usize = 8192;
    let pressure = if pressure_kind.is_none() {
        String::new()
    } else if pressure_kind == Some(true) {
        (0..VALUE_ARENA_BUDGET).fold(String::new(), |mut source, index| {
            writeln!(source, "const filler{index}: number = 0;").expect("fixture source");
            source
        })
    } else {
        if root {
            "scalar.pressure();"
        } else {
            "left.other.pressure();"
        }
        .repeat(CONSTRUCTOR_WORK_BUDGET)
    };
    if root {
        let writes = format!("value = new Sub(); {pressure} value = new Other(); value.after();");
        let writes = if nested {
            format!("if(inner) {{ {writes} }}")
        } else {
            writes
        };
        return format!(
            "import {{ C, Other }} from './classes';
            declare const flag: boolean; declare const inner: boolean;
            class Sub extends C {{}}
            let value = new Sub(); const scalar = value; value.before();
            if(flag) {{ {writes} }} else {{ scalar.alternate(); }}
            value.joined(); scalar.snapshot(); new C().direct();"
        );
    }
    let writes = "left.other = new Other(); right.other = new Other(); left.other = new Sub(); right.other.after();";
    let writes = if nested {
        format!("if(inner) {{ {writes} }}")
    } else {
        writes.to_string()
    };
    format!(
        "import {{ C, Other }} from './classes';
        declare const flag: boolean; declare const inner: boolean;
        class Sub extends C {{}}
        const left = {{ other: new Sub() }}; const right = {{ other: new Sub() }};
        const alias = right; const scalar = left.other;
        left.other.before(); {pressure}
        if(flag) {{ {writes} }} else {{ alias.other.alternate(); }}
        alias.other.joined(); scalar.snapshot(); new C().direct();"
    )
}

#[test]
fn ordinary_subclass_refusal_preserves_shared_slots_and_saved_values() {
    let members = [
        "after",
        "alternate",
        "before",
        "dead",
        "direct",
        "joined",
        "pressure",
        "snapshot",
    ];
    for pressure_kind in [None, Some(false), Some(true)] {
        for (nested, root) in [(false, false), (true, false), (false, true), (true, true)] {
            let source = subclass_refusal_source(pressure_kind, nested, root);
            let classes = "export class C { before() {} after() {} alternate() {} snapshot() {} direct() {} joined() {} pressure() {} dead() {} } export class Other { after() {} }";
            let directory = tempfile::tempdir().expect("fixture directory");
            for (name, content) in [
                (
                    "package.json",
                    r#"{"name":"constructor-refusal","type":"module","main":"index.ts"}"#,
                ),
                ("classes.ts", classes),
                ("index.ts", source.as_str()),
            ] {
                std::fs::write(directory.path().join(name), content).expect("fixture file");
            }
            let mut config = create_config(directory.path().to_path_buf());
            config.rules.unused_class_members = fallow_config::Severity::Error;
            let results = fallow_core::analyze(&config).expect("refusal analysis");
            assert!(results.unresolved_imports.is_empty());
            let info = fallow_extract::parse_from_content(
                fallow_types::discover::FileId(0),
                std::path::Path::new("index.ts"),
                &source,
            );
            for member in members {
                let expected = matches!(member, "alternate" | "before" | "direct" | "snapshot")
                    || member == "pressure" && pressure_kind == Some(false)
                    || member == "joined" && pressure_kind.is_none();
                let extracted = info
                    .member_accesses
                    .iter()
                    .any(|access| access.object == "C" && access.member == member);
                let unused = results.unused_class_members.iter().any(|finding| {
                    finding.member.parent_name == "C" && finding.member.member_name == member
                });
                assert_eq!(
                    extracted, expected,
                    "pressure={pressure_kind:?}, nested={nested}, root={root}, extraction {member}"
                );
                assert_eq!(
                    !unused, expected,
                    "pressure={pressure_kind:?}, nested={nested}, root={root}, public {member}"
                );
            }
            assert!(!info.whole_object_uses.iter().any(|name| name == "C"));
        }
    }
}

fn requirement_alias_source(length: usize, subclass: bool, operation: &str) -> String {
    use std::fmt::Write;

    let target = if subclass { "Sub" } else { "C" };
    let mut source = format!(
        "import {{C, Other}} from './classes'; class Sub extends C {{}}\nfunction scan(items: {{ slot: {target} }}[]) {{ const saved = items[0].slot; saved.before();\n"
    );
    if !subclass {
        source = source.replace("class Sub extends C {}", "");
    }
    source.push_str("const a0=items;\n");
    for index in 1..length {
        writeln!(source, "const a{index}=a{};", index - 1).expect("alias source");
    }
    writeln!(
        source,
        "const last=a{}; {operation} }} scan(unknown()); new C().direct();",
        length - 1
    )
    .expect("alias observation");
    source
}

fn assert_requirement_alias_projection(source: &str, unused: &[&str]) {
    let directory = tempfile::tempdir().expect("alias fixture directory");
    std::fs::write(
        directory.path().join("package.json"),
        r#"{"name":"requirement-alias","type":"module","main":"index.ts"}"#,
    )
    .expect("alias fixture package");
    std::fs::write(directory.path().join("index.ts"), source).expect("alias source");
    std::fs::write(directory.path().join("classes.ts"),
        "export class C { before(){} used(){} snapshot(){} wrong(){} direct(){} dead(){} } export class Other { used(){} wrong(){} }")
        .expect("alias classes");
    let mut config = create_config(directory.path().to_path_buf());
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("alias analysis");
    assert!(
        results.unresolved_imports.is_empty(),
        "unresolved alias imports"
    );
    let mut actual: Vec<_> = results
        .unused_class_members
        .iter()
        .filter(|finding| finding.member.parent_name == "C")
        .map(|finding| finding.member.member_name.as_str())
        .collect();
    actual.sort_unstable();
    assert_eq!(
        actual,
        unused,
        "alias member projection, bytes={}, subclass={}",
        source.len(),
        source.contains("class Sub extends")
    );
}

#[test]
fn indexed_requirement_aliases_preserve_long_chain_ownership() {
    for subclass in [false, true] {
        for length in [16, 256, 2048] {
            for (operation, unused) in [
                (
                    "let cycle=a0; cycle=last; cycle[0].slot.used(); saved.snapshot();",
                    &["dead", "wrong"][..],
                ),
                (
                    "last[0].slot = new Other(); items[0].slot.wrong(); saved.snapshot();",
                    &["dead", "used", "wrong"][..],
                ),
                (
                    "items = unknown(); last[0].slot.used(); saved.snapshot();",
                    &["dead", "wrong"][..],
                ),
                (
                    "{ const last = [{ slot: new Other() }]; last[0].slot.wrong(); } a0[0].slot.used(); saved.snapshot();",
                    &["dead", "wrong"][..],
                ),
            ] {
                assert_requirement_alias_projection(
                    &requirement_alias_source(length, subclass, operation),
                    unused,
                );
            }
        }
    }
}

#[test]
fn indexed_requirement_node_refusal_keeps_prior_observations() {
    const REQUIREMENT_NODE_BUDGET: usize = 8192;
    for subclass in [false, true] {
        let source = requirement_alias_source(
            REQUIREMENT_NODE_BUDGET,
            subclass,
            "last[0].slot = new Other(); items[0].slot.wrong(); saved.snapshot();",
        )
        .replace("saved.before();\n", "saved.before(); {\n")
        .replace("} scan(unknown());", "} } scan(unknown());");
        assert_requirement_alias_projection(&source, &["dead", "snapshot", "used", "wrong"]);
    }
}
