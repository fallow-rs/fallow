use super::common::{create_config, fixture_path};

fn assert_producer_projection(fixture: &str) {
    let mut config = create_config(fixture_path(fixture));
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let result = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        result.unresolved_imports.is_empty(),
        "{fixture}: unresolved imports"
    );
    let mut unused: Vec<_> = result
        .unused_class_members
        .iter()
        .filter(|finding| finding.member.parent_name == "C")
        .map(|finding| finding.member.member_name.as_str())
        .collect();
    unused.sort_unstable();
    assert_eq!(
        unused,
        ["dead", "goStatic", "mixinOnly", "unknownOnly"],
        "{fixture}: only the reached go and direct members are used"
    );
}

#[test]
fn imported_owner_property_keeps_its_declaring_module() {
    assert_producer_projection("class-producer-imported-owner-shape");
}

#[test]
fn direct_inner_factory_call_keeps_its_return_class() {
    assert_producer_projection("class-producer-factory-property");
}

#[test]
fn aliased_inner_factory_call_keeps_its_return_class() {
    assert_producer_projection("class-producer-factory-property-alias");
}

#[test]
fn local_owner_property_remains_supported() {
    assert_producer_projection("class-producer-local-owner-control");
}

#[test]
fn direct_construction_property_remains_supported() {
    assert_producer_projection("class-producer-new-property-control");
}

fn assert_source_projection(label: &str, factory: &str, used: bool, same_file: bool) {
    assert_source_projection_with_consumer(
        label,
        factory,
        used,
        same_file,
        "const ui = createUi(); ui.page.go();",
    );
}

fn assert_source_projection_with_consumer(
    label: &str,
    factory: &str,
    used: bool,
    same_file: bool,
    consumer: &str,
) {
    let directory = tempfile::tempdir().expect("producer fixture setup");
    let root = directory.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"producer-test","type":"module","main":"index.ts"}"#,
    )
    .expect("fixture package");
    std::fs::write(root.join("c.ts"), "export class C { go() {} direct() {} dead() {} static goStatic() {} mixinOnly() {} unknownOnly() {} static create(): C { return new C(); } }").expect("fixture class");
    std::fs::write(root.join("maker.ts"), "import { C } from './c'; export const make = (): C => new C(); export async function asyncMake(): Promise<C> { return new C(); } export function* generate() { yield new C(); }").expect("fixture producer");
    std::fs::write(root.join("barrel.ts"), "export { make as renamed } from './maker'; export { Owner as RenamedOwner } from './owner';").expect("fixture barrel");
    std::fs::write(
        root.join("owner.ts"),
        "import { C } from './c'; export class Owner { readonly widget: C = new C(); }",
    )
    .expect("fixture owner");
    std::fs::write(
        root.join("decoy.ts"),
        "export class C { go() {} direct() {} dead() {} }",
    )
    .expect("decoy class");
    let index = if same_file {
        "import { exercise } from './factory'; exercise(); import { C } from './c'; new C().direct();".to_string()
    } else {
        format!(
            "import {{ createUi }} from './factory'; {{ {consumer} }} import {{ C }} from './c'; new C().direct();"
        )
    };
    let factory = if same_file {
        format!("{factory}\nexport function exercise() {{ {consumer} }}")
    } else {
        factory.to_string()
    };
    let capture = label.ends_with("-capture");
    let label = label.strip_suffix("-capture").unwrap_or(label);
    let factory = if capture {
        format!(
            "{factory}\nimport {{ C as CaptureParent }} from './c'; class Capture extends CaptureParent {{}}"
        )
    } else {
        factory
    };
    let index = if capture {
        format!(
            "{index}\nimport {{ C as CaptureParent }} from './c'; class Capture extends CaptureParent {{}}"
        )
    } else {
        index
    };
    std::fs::write(root.join("factory.ts"), factory).expect("fixture factory");
    std::fs::write(root.join("index.ts"), index).expect("fixture entry");
    let mut config = create_config(root.to_path_buf());
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let result = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        result.unresolved_imports.is_empty(),
        "{label}: unresolved import"
    );
    let mut unused: Vec<_> = result
        .unused_class_members
        .iter()
        .filter(|finding| {
            finding.member.parent_name == "C" && finding.member.path.ends_with("c.ts")
        })
        .map(|finding| finding.member.member_name.as_str())
        .collect();
    unused.sort_unstable();
    let mut expected = vec!["dead", "goStatic", "mixinOnly", "unknownOnly"];
    if !used {
        expected.push("go");
    }
    if label != "static-class" {
        expected.push("create");
    }
    expected.sort_unstable();
    if label == "declaring-module-collision" {
        let mut decoy: Vec<_> = result
            .unused_class_members
            .iter()
            .filter(|finding| finding.member.path.ends_with("decoy.ts"))
            .map(|finding| finding.member.member_name.as_str())
            .collect();
        decoy.sort_unstable();
        assert_eq!(
            decoy,
            ["dead", "go"],
            "decoy only receives its direct control"
        );
    }
    assert_eq!(
        unused, expected,
        "{label}: exact C projection; same_file={same_file}"
    );
}

const KNOWN_PRODUCERS: &[(&str, &str)] = &[
    (
        "imported-call",
        "import { make } from './maker'; export const createUi = () => ({ page: make() });",
    ),
    (
        "imported-result-alias",
        "import { make } from './maker'; export const createUi = () => { const page = make(); const saved = page; return { page: saved }; };",
    ),
    (
        "reexport-call",
        "import { renamed as make } from './barrel'; export const createUi = () => ({ page: make() });",
    ),
    (
        "reexport-owner",
        "import { RenamedOwner as Owner } from './barrel'; export const createUi = () => { const owner = new Owner(); return { page: owner.widget }; };",
    ),
    (
        "declaring-module-collision",
        "import { Owner } from './owner'; import { C } from './decoy'; new C().direct(); export const createUi = () => { const owner = new Owner(); return { page: owner.widget }; };",
    ),
    (
        "static-class",
        "import { C } from './c'; export const createUi = () => ({ page: C.create() });",
    ),
    (
        "local-static",
        "import { C } from './c'; export class Local extends C { static build(): Local { return new Local(); } } export const createUi = () => ({ page: Local.build() });",
    ),
    (
        "forward-local",
        "import { C } from './c'; export const createUi = () => ({ page: make() }); function make(): C { return new C(); }",
    ),
    (
        "local-return-alias",
        "import { C } from './c'; function make() { const instance = new C(); return instance; } export const createUi = () => ({ page: make() });",
    ),
    (
        "typed-property-alias",
        "import { Owner } from './owner'; export const createUi = () => { const owner = new Owner(); const page = owner.widget; return { page }; };",
    ),
    (
        "returned-object-no-write",
        "import { make } from './maker'; export const createUi = () => { const value = { page: make() }; return value; };",
    ),
    (
        "property-snapshot",
        "import { make } from './maker'; export const createUi = () => { const value = { page: make() }; const saved = value.page; value.page = { go() {} }; return { page: saved }; };",
    ),
    (
        "object-alias-no-write",
        "import { make } from './maker'; export const createUi = () => { const value = { page: make() }; const alias = value; return { page: alias.page }; };",
    ),
    (
        "root-rebind-snapshot",
        "import { make } from './maker'; export const createUi = () => { let value = { page: make() }; const alias = value; value = { page: { go() {} } }; return { page: alias.page }; };",
    ),
    (
        "untouched-sibling",
        "import { make } from './maker'; export const createUi = () => { const value = { page: make(), other: 0 }; value.other = 1; return { page: value.page }; };",
    ),
    (
        "sibling-static-key",
        "import { make } from './maker'; export const createUi = () => { const value = { page: make(), other: 0 }; value['other'] = 1; return { page: value.page }; };",
    ),
    (
        "nested-property",
        "import { make } from './maker'; export const createUi = () => { const value = { nested: { page: make() } }; return { page: value.nested.page }; };",
    ),
    (
        "owner-alias",
        "import { Owner } from './owner'; export const createUi = () => { const owner = new Owner(); const alias = owner; return { page: alias.widget }; };",
    ),
    (
        "callee-alias",
        "import { make } from './maker'; const alias = make; export const createUi = () => ({ page: alias() });",
    ),
    (
        "saved-call",
        "import { C } from './c'; const make = (): C => new C(); export const createUi = () => { let page = make(); const saved = page; page = unknown(); return { page: saved }; }; function unknown() { return { go() {} }; }",
    ),
];

#[test]
fn property_producer_sources_keep_their_module_and_binding() {
    for &(label, source) in KNOWN_PRODUCERS {
        for same_file in [false, true] {
            assert_source_projection(label, source, true, same_file);
            assert_source_projection(&format!("{label}-capture"), source, true, same_file);
        }
    }
}

const UNKNOWN_PRODUCERS: &[(&str, &str)] = &[
    (
        "async-inner",
        "import { asyncMake } from './maker'; export const createUi = () => ({ page: asyncMake() });",
    ),
    (
        "generator-inner",
        "import { generate } from './maker'; export const createUi = () => ({ page: generate() });",
    ),
    (
        "shadowed-callee",
        "import { make } from './maker'; export function createUi(make = () => ({ go() {} })) { return { page: make() }; }",
    ),
    (
        "replaced-alias",
        "import { make } from './maker'; export const createUi = () => { let page = make(); page = { go() {} }; return { page }; };",
    ),
    (
        "optional-replacement",
        "import { make } from './maker'; export const createUi = () => { let page = make(); if (flag) page = { go() {} }; return { page }; }; declare const flag: boolean;",
    ),
    (
        "duplicate",
        "import { make } from './maker'; export const createUi = () => ({ page: make(), page: { go() {} } });",
    ),
    (
        "spread",
        "import { make } from './maker'; declare const unknown: object; export const createUi = () => ({ page: make(), ...unknown });",
    ),
    (
        "async-outer",
        "import { make } from './maker'; export const createUi = async () => ({ page: make() });",
    ),
    (
        "conditional-outer",
        "import { make } from './maker'; declare const flag: boolean; export function createUi() { if (flag) return { page: make() }; }",
    ),
    (
        "returned-object-write",
        "import { make } from './maker'; export const createUi = () => { const value = { page: make() }; value.page = { go() {} }; return value; };",
    ),
    (
        "shared-alias-write",
        "import { make } from './maker'; export const createUi = () => { const value = { page: make() }; const alias = value; alias.page = { go() {} }; return { page: value.page }; };",
    ),
    (
        "shared-owner-write",
        "import { make } from './maker'; export const createUi = () => { const value = { page: make() }; const alias = value; value.page = { go() {} }; return { page: alias.page }; };",
    ),
    (
        "unknown-key-write",
        "import { make } from './maker'; declare const key: string; export const createUi = () => { const value = { page: make() }; value[key] = { go() {} }; return { page: value.page }; };",
    ),
    (
        "parent-write",
        "import { make } from './maker'; export const createUi = () => { const value = { nested: { page: make() } }; value.nested = { page: { go() {} } }; return { page: value.nested.page }; };",
    ),
    (
        "optional-slot-write",
        "import { make } from './maker'; declare const flag: boolean; export const createUi = () => { const value = { page: make() }; const alias = value; if (flag) value.page = { go() {} }; return { page: alias.page }; };",
    ),
    (
        "callee-rebound",
        "import { make } from './maker'; let alias = make; alias = () => ({ go() {} }); export const createUi = () => ({ page: alias() });",
    ),
    (
        "local-return-replaced",
        "import { C } from './c'; function make() { let instance = new C(); instance = { go() {} }; return instance; } export const createUi = () => ({ page: make() });",
    ),
    (
        "local-return-annotation-only",
        "import { C } from './c'; function make() { let instance: C; return instance; } export const createUi = () => ({ page: make() });",
    ),
    (
        "dotted-key",
        "import { make } from './maker'; export const createUi = () => { const value = { 'nested.page': make() }; return { page: value.nested.page }; };",
    ),
    (
        "local-class-shadowed-owner",
        "import { Owner } from './owner'; export function createUi() { class Owner { widget = { go() {} }; } const owner = new Owner(); return { page: owner.widget }; }",
    ),
    (
        "shadowed-owner",
        "import { Owner } from './owner'; export function createUi(Owner = class { widget = { go() {} }; }) { const owner = new Owner(); return { page: owner.widget }; }",
    ),
];

#[test]
fn property_producer_unknown_or_replaced_values_do_not_credit_class() {
    for &(label, source) in UNKNOWN_PRODUCERS {
        for same_file in [false, true] {
            assert_source_projection(label, source, false, same_file);
            assert_source_projection(&format!("{label}-capture"), source, false, same_file);
        }
    }
}

#[test]
fn private_local_static_factory_remains_usable_in_its_own_module() {
    let source = "import { C } from './c'; class Local extends C { static build(): Local { return new Local(); } } export const createUi = () => ({ page: Local.build() });";
    for label in ["private-static", "private-static-capture"] {
        assert_source_projection(label, source, true, true);
    }
}

#[test]
fn shadowed_object_producer_does_not_borrow_module_shape() {
    for source in [
        "import { make } from './maker'; export const createUi = () => ({ page: make() });",
        "import { C } from './c'; const make = (): C => new C(); export const createUi = () => ({ page: make() });",
    ] {
        for same_file in [false, true] {
            for label in ["shadowed-outer", "shadowed-outer-capture"] {
                assert_source_projection_with_consumer(
                    label,
                    source,
                    false,
                    same_file,
                    "const createUi = () => ({ page: { go() {} } }); const ui = createUi(); ui.page.go();",
                );
            }
        }
    }
}

#[test]
fn property_consumers_keep_scoped_values_and_source_time_reads() {
    let factory =
        "import { make } from './maker'; export const createUi = () => ({ page: make() });";
    for (label, consumer, used) in [
        (
            "receiver-callee-alias",
            "const make = createUi; const ui = make(); ui.page.go();",
            true,
        ),
        (
            "receiver-alias",
            "const ui = createUi(); const saved = ui; saved.page.go();",
            true,
        ),
        (
            "receiver-rebind",
            "let ui = createUi(); const saved = ui; ui = { page: { go() {} } }; saved.page.go();",
            true,
        ),
        (
            "receiver-before-write",
            "const ui = createUi(); ui.page.go(); ui.page = { go() {} };",
            true,
        ),
        (
            "receiver-slot-write",
            "const ui = createUi(); ui.page = { go() {} }; ui.page.go();",
            false,
        ),
        (
            "receiver-alias-write",
            "const ui = createUi(); const alias = ui; alias.page = { go() {} }; ui.page.go();",
            false,
        ),
        (
            "receiver-root-write",
            "let ui = createUi(); ui = { page: { go() {} } }; ui.page.go();",
            false,
        ),
        (
            "receiver-shadow",
            "const ui = createUi(); { const ui = { page: { go() {} } }; ui.page.go(); }",
            false,
        ),
        (
            "receiver-optional-write",
            "const ui = createUi(); if (globalThis.flag) ui.page = { go() {} }; ui.page.go();",
            false,
        ),
        (
            "receiver-alternate-read",
            "const ui = createUi(); if (globalThis.flag) ui.page = { go() {} }; else ui.page.go();",
            true,
        ),
        (
            "receiver-unknown-write",
            "const ui = createUi(); ui[globalThis.key] = { go() {} }; ui.page.go();",
            false,
        ),
    ] {
        for same_file in [false, true] {
            assert_source_projection_with_consumer(label, factory, used, same_file, consumer);
            assert_source_projection_with_consumer(
                &format!("{label}-capture"),
                factory,
                used,
                same_file,
                consumer,
            );
        }
    }
}

fn assert_typed_owner_slot(body: &str, used: bool, capture: bool) {
    let directory = tempfile::tempdir().expect("typed owner fixture");
    let root = directory.path();
    let capture_source = if capture {
        " import { C } from './c'; class Capture extends C {}"
    } else {
        ""
    };
    let sources = [
        ("package.json", r#"{"name":"typed-owner-slots","type":"module","main":"index.ts"}"#.to_string()),
        ("c.ts", "export class C { go() {} direct() {} dead() {} } export class Other { go() {} direct() {} dead() {} }".to_string()),
        ("owner.ts", "import { C } from './c'; export class Owner { widget: C = new C(); }".to_string()),
        ("factory.ts", format!("import {{ Owner }} from './owner'; import {{ Other }} from './c'; declare function opaque(): unknown; export function createUi() {{ const owner = new Owner(); {body} }}{capture_source}")),
        ("index.ts", "import { createUi } from './factory'; import { C, Other } from './c'; const ui = createUi(); ui.page.go(); new C().direct(); new Other().direct();".to_string()),
    ];
    for (name, source) in sources {
        std::fs::write(root.join(name), source).expect("typed owner source");
    }
    let mut config = create_config(root.to_path_buf());
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let result = fallow_core::analyze(&config).expect("typed owner analysis");
    assert!(result.unresolved_imports.is_empty(), "{body}");
    let mut unused: Vec<_> = result
        .unused_class_members
        .iter()
        .map(|finding| {
            format!(
                "{}.{}",
                finding.member.parent_name, finding.member.member_name
            )
        })
        .collect();
    unused.sort_unstable();
    let mut expected = vec!["C.dead", "Other.dead", "Other.go"];
    if !used {
        expected.push("C.go");
    }
    expected.sort_unstable();
    assert_eq!(unused, expected, "capture={capture}; {body}");
}

#[test]
fn typed_owner_replaced_slots_do_not_reuse_original_property_type() {
    for capture in [false, true] {
        for body in [
            "owner.widget = new Other(); return { page: owner.widget };",
            "const alias = owner; alias.widget = new Other(); return { page: owner.widget };",
        ] {
            assert_typed_owner_slot(body, false, capture);
        }
    }
}

#[test]
fn typed_owner_unmaterialized_unknown_writes_leave_barriers() {
    for capture in [false, true] {
        for body in [
            "owner.widget = opaque(); return { page: owner.widget };",
            "const alias = owner; alias.widget = opaque(); return { page: owner.widget };",
            "owner[globalThis.key] = opaque(); return { page: owner.widget };",
            "if (globalThis.flag) owner.widget = opaque(); return { page: owner.widget };",
        ] {
            assert_typed_owner_slot(body, false, capture);
        }
    }
}

#[test]
fn typed_owner_slot_reads_keep_incoming_and_saved_values() {
    for capture in [false, true] {
        for body in [
            "return { page: owner.widget };",
            "const alias = owner; return { page: alias.widget };",
            "const saved = owner.widget; owner.widget = new Other(); return { page: saved };",
            "const saved = owner.widget; if (globalThis.flag) owner.widget = opaque(); return { page: saved };",
            "if (globalThis.flag) owner.widget = opaque(); else owner.widget.go(); return { page: opaque() };",
        ] {
            assert_typed_owner_slot(body, true, capture);
        }
    }
}
