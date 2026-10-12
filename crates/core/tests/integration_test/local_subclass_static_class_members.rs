use super::common::{create_config, fixture_path};

/// A member reached through a LOCAL (non-exported) subclass must credit the class
/// that declares it.
///
/// `class Sub extends Base {}` never becomes an export, so the analyze layer's
/// import/export map cannot resolve `Sub.someStatic`, and the heritage
/// `parent -> children` map is built from exports alone. Every member reached only
/// through the subclass was therefore reported unused. Exporting the subclass makes
/// the identical code resolve, which is what pinned the diagnosis.
///
/// The fixture is deliberately cross-file: the subclass lives one module away from
/// the base, which is what real code looks like and what a same-file fixture cannot
/// exercise. `UrlSyncManager.trulyDead` and `instanceTrulyDead` are the non-vacuous
/// controls proving the detector still fires.
fn unused_members(fixture: &str) -> Vec<String> {
    let root = fixture_path(fixture);
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    results
        .unused_class_members
        .iter()
        .map(|member| {
            format!(
                "{}.{}",
                member.member.parent_name, member.member.member_name
            )
        })
        .collect()
}

#[test]
fn local_subclass_credits_members_on_the_declaring_class() {
    let unused = unused_members("local-subclass-static-class-members");

    for member in [
        // Static method, static passed as a value, and a static arrow property.
        // There is no method/arrow divergence: propagation is on member names.
        "calledViaSub",
        "passedViaSub",
        "arrowViaSub",
        // A two-level local chain.
        "viaGrandchild",
        // Reached through the base directly; never affected by this bug.
        "passedViaBase",
        // `const instance = new MapUrlSyncManager(); instance.instanceViaSub()`.
        // The receiver must retain its actual instance binding through propagation.
        "instanceViaSub",
        // A chain of 18 local classes. Cycle detection, not a depth cap, terminates
        // the walk; a fixed cap would abstain here and report this falsely.
        "viaDeepChain",
        // The subclass declares its own `shadowedOnSub`, so this credit on the base
        // is an over-credit. It is the accepted direction: a false negative, never a
        // false positive.
        "shadowedOnSub",
    ] {
        assert!(
            !unused.contains(&format!("UrlSyncManager.{member}")),
            "UrlSyncManager.{member} is reached through a local subclass and must be \
             credited, found: {unused:?}"
        );
    }

    for dead in ["trulyDead", "instanceTrulyDead"] {
        assert!(
            unused.contains(&format!("UrlSyncManager.{dead}")),
            "UrlSyncManager.{dead} is never referenced and must still be reported, \
             otherwise this fixture passes vacuously: {unused:?}"
        );
    }
}

/// `class Sub extends mixin(Base) {}` records no superclass, because a mixin call is
/// not a bare identifier. The walk abstains and `viaMixin` stays reported.
///
/// This is a known, pre-existing false positive, not one this change introduces:
/// crediting through a mixin would be a guess, since the mixin may redefine what the
/// subclass exposes. Pinned here so a future change to superclass extraction has to
/// decide this case deliberately rather than by accident.
#[test]
fn mixin_superclass_abstains_rather_than_guessing() {
    let unused = unused_members("local-subclass-static-class-members");

    assert!(
        unused.contains(&"UrlSyncManager.viaMixin".to_string()),
        "a mixin superclass carries no resolvable name, so the walk must abstain; \
         if this now passes, superclass extraction changed and the abstention should \
         be revisited: {unused:?}"
    );
}

/// Namespace subclasses use the declaring export through the qualified member fact.
#[test]
fn namespace_qualified_local_subclass_credits_declaring_members() {
    let unused = unused_members("local-subclass-static-class-members");

    for member in [
        "viaNamespaceBase",
        "instanceViaNamespaceBase",
        "passedViaNamespaceBase",
        "viaNamespaceChain",
    ] {
        assert!(
            !unused.contains(&format!("UrlSyncManager.{member}")),
            "UrlSyncManager.{member} is reached through a namespace subclass: {unused:?}"
        );
    }
    for member in ["trulyDead", "instanceTrulyDead", "viaMixin"] {
        assert!(
            unused.contains(&format!("UrlSyncManager.{member}")),
            "unreached UrlSyncManager.{member} must remain reportable: {unused:?}"
        );
    }
}

fn assert_subclass_receiver_identity(fixture: &str) {
    let unused = unused_members(fixture);
    for member in [
        "positive",
        "instance",
        "functionLocalInstance",
        "typedParameter",
        "aliasInstance",
        "heldInstance",
        "restoredLocal",
        "restoredRest",
        "restoredCatch",
        "restoredLoop",
    ] {
        assert!(
            !unused.contains(&format!("C.{member}")),
            "proven receiver must credit C.{member}: {unused:?}"
        );
    }
    for member in [
        "shadowOnly",
        "unknownOnly",
        "dead",
        "localStatic",
        "localInstance",
        "restStatic",
        "restInstance",
        "catchStatic",
        "catchInstance",
        "loopStatic",
        "loopInstance",
    ] {
        assert!(
            unused.contains(&format!("C.{member}")),
            "unrelated shadowed receiver must not credit C.{member}: {unused:?}"
        );
    }
}

#[test]
fn bare_subclass_receiver_shadowing_preserves_unused_members() {
    assert_subclass_receiver_identity("bare-subclass-receiver-shadowing");
}

#[test]
fn namespace_subclass_receiver_shadowing_preserves_unused_members() {
    assert_subclass_receiver_identity("namespace-subclass-receiver-shadowing");
}

#[test]
fn namespace_subclass_renamed_barrel_credits_only_declaring_class() {
    let unused = unused_members("local-subclass-static-class-members");
    assert!(
        !unused.contains(&"UrlSyncManager.viaRenamedBarrel".to_string()),
        "renamed origin must be credited: {unused:?}"
    );
    for member in [
        "UrlSyncManager.trulyDead",
        "UnrelatedManager.viaRenamedBarrel",
        "UnrelatedManager.unrelatedDead",
    ] {
        assert!(
            unused.contains(&member.to_string()),
            "unreached declaring and sibling members remain reportable: {unused:?}"
        );
    }
}

#[test]
fn local_subclass_existing_binding_routes_remain_credited() {
    let unused = unused_members("local-subclass-binding-preservation");
    for member in [
        "nested",
        "destructured",
        "annotated",
        "field",
        "mapped",
        "beforeDeclaration",
    ] {
        assert!(
            !unused.contains(&format!("C.{member}")),
            "existing proven binding must credit C.{member}: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"C.dead".to_string()),
        "dead sibling remains reportable: {unused:?}"
    );
}

#[test]
fn local_subclass_container_identity_preserves_old_objects_and_invalidates_mutations() {
    let unused = unused_members("local-subclass-container-identity");
    for member in ["live", "rebindUsed", "beforeMutation", "ancestorUsed"] {
        assert!(
            !unused.contains(&format!("C.{member}")),
            "original object must retain C.{member} credit: {unused:?}"
        );
    }
    for member in ["dead", "rebindWrong", "mutationWrong", "ancestorWrong"] {
        assert!(
            unused.contains(&format!("C.{member}")),
            "changed unknown object must not credit C.{member}: {unused:?}"
        );
    }
}

#[test]
fn local_subclass_review_boundaries_credit_only_current_proven_receivers() {
    let mut unused = unused_members("local-subclass-review-boundaries");
    unused.sort();
    assert_eq!(
        unused,
        [
            "C.dead",
            "C.duplicateWrong",
            "C.mapWrong",
            "C.namedDynamicWrong",
            "C.namedWrong",
            "C.thisWrong",
            "C.unknownWrong",
        ]
    );
}

#[test]
fn local_subclass_late_producers_preserve_proven_results_and_reject_changed_values() {
    let mut unused = unused_members("local-subclass-late-producer-preservation");
    unused.sort();
    assert_eq!(
        unused,
        [
            "C.childWrong",
            "C.dead",
            "C.factoryWrong",
            "C.independentWrong",
            "C.promiseWrong",
            "C.rootWrong",
            "C.staticWrong",
            "C.unsupportedAlias",
            "C.unsupportedObject",
            "C.unsupportedStructural",
        ]
    );
}

#[test]
fn local_subclass_late_review_keeps_contracts_and_invalidates_written_receivers() {
    for fixture in [
        "local-subclass-late-review-boundaries",
        "bare-subclass-late-review-boundaries",
    ] {
        let mut unused = unused_members(fixture);
        unused.sort();
        assert_eq!(
            unused,
            [
                "C.constructorWrong",
                "C.containerWrong",
                "C.dead",
                "C.defaultWrong",
                "C.factoryWrong",
                "C.getterWrong",
                "C.independentThisWrong",
                "C.iterationWrong",
                "C.nestedWrong",
                "C.promiseWrong",
                "C.receiverWrong",
                "C.unsupportedPromiseAll",
                "Handler.unusedHandler"
            ],
            "{fixture}"
        );
    }
}

#[test]
fn ordinary_conditional_first_write_preserves_reachable_class_members() {
    for capture in [false, true] {
        for (label, body, alpha_unused) in [
            ("if", "if(flag){holder.property=new Beta();}", false),
            (
                "narrowing",
                "if(flag && holder instanceof Object){holder.property=new Beta();}",
                false,
            ),
            (
                "ternary",
                "flag ? holder.property=new Beta() : undefined;",
                false,
            ),
            ("and", "flag && (holder.property=new Beta());", false),
            ("or", "flag || (holder.property=new Beta());", false),
            ("nullish", "value ?? (holder.property=new Beta());", false),
            (
                "while",
                "while(flag){holder.property=new Beta();break;}",
                false,
            ),
            (
                "for",
                "for(let i=0;i<(flag?1:0);i++){holder.property=new Beta();}",
                false,
            ),
            (
                "for-of",
                "for(const item of (flag?[1]:[])){holder.property=new Beta();}",
                false,
            ),
            (
                "for-in",
                "for(const key in (flag?{ x: 1 }:{})){holder.property=new Beta();}",
                false,
            ),
            (
                "switch",
                "switch(flag){case true:holder.property=new Beta();break;}",
                false,
            ),
            ("straight", "holder.property=new Beta();", true),
            (
                "do-while",
                "do{holder.property=new Beta();}while(false);",
                true,
            ),
        ] {
            let temp = tempfile::tempdir().expect("temporary fixture");
            let root = temp.path();
            std::fs::write(
                root.join("package.json"),
                r#"{"name":"conditional-binding","main":"index.ts"}"#,
            )
            .unwrap();
            std::fs::write(root.join("services.ts"), "export class Alpha { used() {} dead() {} } export class Beta { used() {} dead() {} }").unwrap();
            std::fs::write(
                root.join("base.ts"),
                "export class Base { static used() {} static dead() {} }",
            )
            .unwrap();
            let marker = if capture {
                "import * as ns from './base';class Sub extends ns.Base{}Sub.used();"
            } else {
                ""
            };
            std::fs::write(root.join("index.ts"), format!("{marker}import {{Alpha,Beta}} from './services';const holder={{property:new Alpha()}};function mutate(flag:boolean,value:unknown){{{body}holder.property.used();}}mutate(process.argv.length>2);")).unwrap();
            let results = fallow_core::analyze(&create_config(root.to_path_buf()))
                .expect("analysis should succeed");
            assert!(results.unresolved_imports.is_empty(), "imports resolve");
            let mut unused = results
                .unused_class_members
                .iter()
                .filter(|m| matches!(m.member.parent_name.as_str(), "Alpha" | "Beta"))
                .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
                .collect::<Vec<_>>();
            unused.sort();
            let expected = if alpha_unused {
                vec!["Alpha.dead", "Alpha.used", "Beta.dead"]
            } else {
                vec!["Alpha.dead", "Beta.dead"]
            };
            assert_eq!(unused, expected, "capture={capture}, {label}");
        }
    }
}

#[test]
fn ordinary_for_update_reaches_later_iteration_members() {
    let mut failures = Vec::new();
    for capture in [false, true] {
        let temp = tempfile::tempdir().expect("temporary fixture");
        let root = temp.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"loop-update-binding","main":"index.ts"}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("services.ts"),
            "export class Alpha { used() {} dead() {} } export class Beta { used() {} dead() {} }",
        )
        .unwrap();
        std::fs::write(
            root.join("base.ts"),
            "export class Base { static used() {} static dead() {} }",
        )
        .unwrap();
        let marker = if capture {
            "import * as ns from './base';class Sub extends ns.Base{}Sub.used();"
        } else {
            ""
        };
        std::fs::write(root.join("index.ts"), format!("{marker}import {{Alpha,Beta}} from './services';const holder={{property:new Alpha()}};function mutate(){{for(let i=0;i<2;holder.property=new Beta(),i++){{holder.property.used();}}}}mutate();")).unwrap();
        let results =
            fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis succeeds");
        assert!(results.unresolved_imports.is_empty(), "imports resolve");
        let mut unused = results
            .unused_class_members
            .iter()
            .filter(|member| matches!(member.member.parent_name.as_str(), "Alpha" | "Beta"))
            .map(|member| {
                format!(
                    "{}.{}",
                    member.member.parent_name, member.member.member_name
                )
            })
            .collect::<Vec<_>>();
        unused.sort_unstable();
        if unused != ["Alpha.dead", "Beta.dead"] {
            failures.push(format!("capture={capture}: {unused:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "loop update must reach the next iteration: {failures:?}"
    );
}

#[test]
fn nested_conditional_root_replacements_do_not_restore_subclass_proof() {
    let mut failures = Vec::new();
    for (label, body) in [
        (
            "outer replacement control",
            "if(flag){holder.other=1;if(other){holder=opaque();}else{holder=opaque();}}else{holder=opaque();}",
        ),
        (
            "both nested outer arms",
            "if(flag){holder.other=1;if(other){holder=opaque();}else{holder=opaque();}}else{holder.other=1;if(other){holder=opaque();}else{holder=opaque();}}",
        ),
    ] {
        let temp = tempfile::tempdir().expect("temporary fixture");
        let root = temp.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"nested-binding","main":"index.ts"}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("base.ts"),
            "export class C { static live() {} before() {} wrong() {} dead() {} }",
        )
        .unwrap();
        std::fs::write(root.join("index.ts"),format!("import * as ns from './base';class Sub extends ns.C {{}}Sub.live();let holder={{item:new Sub(),other:0}};holder.item.before();function opaque(){{return {{item:{{wrong(){{}}}},other:0}};}}const flag=process.argv.length>2;const other=process.argv.length>3;{body}holder.item.wrong();")).unwrap();
        let results =
            fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis succeeds");
        assert!(results.unresolved_imports.is_empty(), "imports resolve");
        let mut unused = results
            .unused_class_members
            .iter()
            .filter(|member| member.member.parent_name == "C")
            .map(|member| member.member.member_name.as_str())
            .collect::<Vec<_>>();
        unused.sort_unstable();
        if unused != ["dead", "wrong"] {
            failures.push(format!("{label}: {unused:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "nested root replacement must invalidate Base proof: {failures:?}"
    );
}

#[test]
fn ordinary_for_update_read_overwrite_boundaries() {
    for capture in [false, true] {
        for (label, update, gamma, beta) in [
            (
                "body input",
                "holder.nested.property.updateRead()",
                true,
                false,
            ),
            (
                "initially unresolved",
                "holder.nested.property.updateRead()",
                true,
                false,
            ),
            (
                "exhaustive ancestors",
                "flag?(holder.nested.property=new Beta()):(holder.nested=new Beta()),holder.nested.property.updateRead()",
                false,
                true,
            ),
            (
                "straight",
                "holder.nested.property=new Beta(),holder.nested.property.updateRead()",
                false,
                true,
            ),
            (
                "optional",
                "flag&&(holder.nested.property=new Beta()),holder.nested.property.updateRead()",
                true,
                true,
            ),
            (
                "unknown root",
                "holder=opaque(),holder.nested.property.updateRead()",
                false,
                false,
            ),
            (
                "unknown parent",
                "holder.nested=opaque(),holder.nested.property.updateRead()",
                false,
                false,
            ),
            (
                "optional unknown",
                "flag&&(holder.nested=opaque()),holder.nested.property.updateRead()",
                true,
                false,
            ),
            (
                "sibling",
                "holder.other=1,holder.nested.property.updateRead()",
                true,
                false,
            ),
        ] {
            assert_for_update_read_members(label, update, capture, gamma, beta);
        }
    }
}

#[test]
fn ordinary_for_update_read_scope_and_timing_boundaries() {
    for capture in [false, true] {
        for (label, update, gamma, beta) in [
            (
                "function shadow",
                "(()=>{const holder={nested:{property:new Beta()}};holder.nested.property.updateRead();})()",
                false,
                true,
            ),
            (
                "parameter shadow",
                "(holder=>{holder.nested.property=new Beta();holder.nested.property.updateRead();})(opaque())",
                false,
                true,
            ),
            (
                "lexical shadow",
                "(class {static {const holder={nested:{property:new Beta()}};holder.nested.property.updateRead();}})",
                false,
                true,
            ),
            (
                "nested function write",
                "(()=>{holder.nested.property=new Beta();}),holder.nested.property.updateRead()",
                true,
                true,
            ),
            (
                "parent before optional",
                "holder.nested=opaque(),flag&&(holder.other=1),holder.nested.property.updateRead()",
                false,
                false,
            ),
            (
                "inside written arm",
                "flag&&(holder.nested.property=new Beta(),holder.nested.property.updateRead())",
                false,
                true,
            ),
            (
                "different siblings",
                "flag?(holder.other=1):(holder.another=2),holder.nested.property.updateRead()",
                true,
                false,
            ),
            (
                "right hand read",
                "holder.nested.property=(holder.nested.property.updateRead(),new Beta())",
                true,
                false,
            ),
        ] {
            assert_for_update_read_members(label, update, capture, gamma, beta);
        }
    }
}

fn assert_for_update_read_members(
    label: &str,
    update: &str,
    capture: bool,
    gamma: bool,
    beta: bool,
) {
    let temp = tempfile::tempdir().expect("temporary fixture");
    let root = temp.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"loop-update-observations","main":"index.ts"}"#,
    )
    .unwrap();
    std::fs::write(root.join("services.ts"),"export class Alpha { updateRead() {} bodyRead() {} dead() {} } export class Beta { updateRead() {} bodyRead() {} dead() {} } export class Gamma { updateRead() {} bodyRead() {} dead() {} }").unwrap();
    std::fs::write(
        root.join("base.ts"),
        "export class Base { static used() {} static dead() {} }",
    )
    .unwrap();
    let marker = if capture {
        "import * as ns from './base';class Sub extends ns.Base{}Sub.used();"
    } else {
        ""
    };
    let initial = if label == "initially unresolved" {
        "opaque()"
    } else {
        "new Alpha()"
    };
    std::fs::write(root.join("index.ts"),format!("{marker}import {{Alpha,Beta,Gamma}} from './services';new Alpha();new Beta();let holder={{nested:{{property:{initial}}},other:0}};function opaque(){{return {{nested:{{property:{{updateRead(){{}}}}}}}};}}function mutate(flag){{for(let i=0;i<2;({update},i++)){{holder.nested.property=new Gamma();holder.nested.property.bodyRead();}}}}mutate(process.argv.length>2);")).unwrap();
    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis succeeds");
    assert!(results.unresolved_imports.is_empty(), "imports resolve");
    for (class, used) in [("Gamma", gamma), ("Beta", beta)] {
        let unused = results
            .unused_class_members
            .iter()
            .any(|m| m.member.parent_name == class && m.member.member_name == "updateRead");
        assert_eq!(unused, !used, "{label}, capture={capture}, {class}");
    }
    for class in ["Alpha", "Beta", "Gamma"] {
        assert!(
            results
                .unused_class_members
                .iter()
                .any(|m| m.member.parent_name == class && m.member.member_name == "dead"),
            "{label}, capture={capture}, {class}.dead"
        );
    }
    assert!(
        !results
            .unused_class_members
            .iter()
            .any(|m| m.member.parent_name == "Gamma" && m.member.member_name == "bodyRead"),
        "body read remains reachable"
    );
}

#[test]
fn local_subclass_jsx_member_tag_credits_declaring_class() {
    let unused = unused_members("local-subclass-jsx-member-tag");
    assert!(
        !unused.contains(&"Base.Item".to_string()),
        "`<Sub.Item />` must credit the static on the imported base: {unused:?}"
    );
    for member in ["Base.Shadowed", "Base.Dead"] {
        assert!(
            unused.contains(&member.to_string()),
            "a shadowed `Sub` tag and an unread static stay reported: {unused:?}"
        );
    }
}

/// Reads on a receiver without a projection proof (here a `number` parameter)
/// must not use the requirement budget of the subclass proofs.
const PROJECTION_PRESSURE_READS: usize = 8192 + 512;

#[test]
fn local_subclass_credit_survives_reads_on_receivers_without_proof() {
    let source_root = fixture_path("local-subclass-binding-preservation");
    let directory = tempfile::tempdir().expect("pressure fixture setup");
    let src = directory.path().join("src");
    std::fs::create_dir(&src).expect("pressure fixture setup");
    std::fs::copy(
        source_root.join("package.json"),
        directory.path().join("package.json"),
    )
    .expect("pressure fixture setup");
    std::fs::copy(source_root.join("src/c.ts"), src.join("c.ts")).expect("pressure fixture setup");
    let original =
        std::fs::read_to_string(source_root.join("src/index.ts")).expect("pressure fixture setup");
    let reads = "n.toFixed();\n".repeat(PROJECTION_PRESSURE_READS);
    let (imports, rest) = original
        .split_once('\n')
        .expect("fixture starts with an import line");
    let padded =
        format!("{imports}\nexport const pad = (n: number): void => {{\n{reads}}};\n{rest}");
    std::fs::write(src.join("index.ts"), padded).expect("pressure fixture setup");

    let config = create_config(directory.path().to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|member| {
            format!(
                "{}.{}",
                member.member.parent_name, member.member.member_name
            )
        })
        .collect();
    assert_eq!(unused, ["C.dead"]);
}
