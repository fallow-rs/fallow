use super::common::create_config;

fn assert_callback_projection(source: &str, used: bool) {
    assert_callback_files(source, used, &[]);
}

fn assert_callback_files(source: &str, used: bool, files: &[(&str, &str)]) {
    let directory = tempfile::tempdir().expect("callback fixture directory");
    let root = directory.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"callback-test","type":"module","main":"index.ts"}"#,
    )
    .expect("package");
    std::fs::write(
        root.join("c.ts"),
        "export class C { go() {} direct() {} dead() {} unknownOnly() {} }",
    )
    .expect("class");
    std::fs::write(root.join("index.ts"), format!("import {{ C }} from './c'; new C().direct(); declare const unrelated: unknown; unrelated.unknownOnly(); {source}")).expect("caller");
    for (path, content) in files {
        std::fs::write(root.join(path), content).expect("supporting module");
    }
    let mut config = create_config(root.to_path_buf());
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let result = fallow_core::analyze(&config).expect("analysis");
    assert!(result.unresolved_imports.is_empty(), "unresolved imports");
    let mut unused: Vec<_> = result
        .unused_class_members
        .iter()
        .filter(|finding| {
            finding.member.parent_name == "C" && finding.member.path.ends_with("c.ts")
        })
        .map(|finding| finding.member.member_name.as_str())
        .collect();
    unused.sort_unstable();
    let expected = if used {
        vec!["dead", "unknownOnly"]
    } else {
        vec!["dead", "go", "unknownOnly"]
    };
    assert_eq!(
        unused, expected,
        "callback producer must credit exactly the reached class member: {source}"
    );
    if root.join("decoy.ts").exists() {
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
            "helper class cannot own the callback result"
        );
    }
}

#[test]
fn callback_wrapper_value_hop_credits_only_proven_class_member() {
    assert_callback_projection(
        "function computed<T>(fn: () => T) { return { value: fn() }; } const result = computed(() => new C()); result.value.go();",
        true,
    );
}

#[test]
fn unbound_callback_wrapper_value_hop_credits_only_proven_class_member() {
    assert_callback_projection(
        "function computed<T>(fn: () => T) { return { value: fn() }; } computed(() => new C()).value.go();",
        true,
    );
}

#[test]
fn local_callback_wrapper_controls() {
    for source in [
        "function computed<T>(fn: () => T) { return { value: fn() }; } const result = computed<C>(() => new C()); result.value.go();",
        "function renamed<T>(fn: () => T) { return { result: fn() }; } const result = renamed(() => { return new C(); }); result.result.go();",
        "const result = { value: new C() }; result.value.go();",
        "const result: { value: C } = { value: new C() }; result.value.go();",
    ] {
        assert_callback_projection(source, true);
    }
    for source in [
        "function computed<T>(fn: () => T) { return { value: { go() {} } }; } const result = computed(() => new C()); result.value.go();",
        "function computed<T>(fn: () => T) { return { value: fn() }; } const result = computed(async () => new C()); result.value.go();",
        "function computed<T>(fn: () => T) { return { value: fn() }; } const result = computed(function* () { return new C(); }); result.value.go();",
        "function computed<T>(fn: () => T) { return { value: fn() }; } const result = computed(() => { function nested() { return new C(); } return {}; }); result.value.go();",
        "function computed<T>(fn: () => T) { return { value: fn() }; } declare const flag: boolean; const result = computed(() => { if (flag) return new C(); }); result.value.go();",
        "function computed<T>(fn: () => T) { return { value: fn() }; } const result = computed(() => new C());",
    ] {
        assert_callback_projection(source, false);
    }
}

#[test]
fn callback_wrapper_saved_property_retains_its_value() {
    assert_callback_projection(
        "function computed<T>(fn: () => T) { return { value: fn() }; } const result = computed(() => new C()); const saved = result.value; result.value = { go() {} }; saved.go();",
        true,
    );
}

#[test]
fn callback_wrapper_alias_mutation_does_not_reuse_original_result() {
    assert_callback_projection(
        "function computed<T>(fn: () => T) { return { value: fn() }; } const result = computed(() => new C()); const alias = result; alias.value = { go() {} }; result.value.go();",
        false,
    );
}

#[test]
fn imported_callback_wrapper_preserves_caller_class_origin() {
    assert_callback_files(
        "import { wrap } from './barrel'; const result = wrap(() => new C()); result.result.go();",
        true,
        &[
            (
                "helper.ts",
                "export function computed<T>(fn: () => T) { return { result: fn() }; }",
            ),
            ("barrel.ts", "export { computed as wrap } from './helper';"),
        ],
    );
}

#[test]
fn vue_computed_uses_canonical_import_and_caller_getter() {
    for source in [
        "import { computed as derive } from 'vue'; const result = derive(() => new C()); result.value.go();",
        "import * as reactivity from '@vue/reactivity'; reactivity.computed(() => new C()).value.go();",
        "import { derive } from './barrel'; const result = derive(() => new C()); result.value.go();",
    ] {
        assert_callback_files(
            source,
            true,
            &[("barrel.ts", "export { computed as derive } from 'vue';")],
        );
    }
    for source in [
        "import { computed } from '@angular/core'; const result = computed(() => new C()); result.value.go();",
        "import { computed } from 'foreign'; const result = computed(() => new C()); result.value.go();",
        "import { computed } from 'vue'; function exercise(computed: any) { computed(() => new C()).value.go(); } exercise(() => ({}));",
        "import { computed } from 'vue'; computed(async () => new C()).value.go();",
    ] {
        assert_callback_projection(source, false);
    }
}

#[test]
fn callback_wrapper_stable_callback_alias() {
    assert_callback_projection(
        "function computed<T>(fn: () => T) { return { value: fn() }; } const callback = () => new C(); const alias = callback; computed(alias).value.go();",
        true,
    );
}

#[test]
fn vue_computed_writable_getter_owns_value() {
    assert_callback_projection(
        "import { computed } from 'vue'; const result = computed({ get: () => new C(), set: (_value: C) => {} }); result.value.go();",
        true,
    );
}

#[test]
fn callback_wrapper_ownership_and_nested_paths() {
    for source in [
        "function wrap<T>(fn: () => T) { return { nested: { result: fn() } }; } const alias = wrap; const result = alias(() => new C()); result.nested.result.go();",
        "const result = wrap(() => new C()); result.value.go(); function wrap<T>(fn: () => T) { return { value: fn() }; }",
        "function wrap<T>(unused: unknown, fn: () => T) { return { value: fn() }; } wrap(1, () => new C()).value.go();",
        "function wrap<T>(fn: () => T) { return { value: fn() }; } let result = wrap(() => new C()); const alias = result; result = { value: { go() {} } }; alias.value.go();",
        "function wrap<T>(fn: () => T) { return { value: fn() }; } const instance = new C(); wrap(() => instance).value.go();",
        "function wrap<T>(fn: () => T) { return { value: fn() }; } declare function unknownValue(): unknown; wrap((): C => unknownValue() as C).value.go();",
    ] {
        assert_callback_projection(source, true);
    }
    for source in [
        "function wrap<T>(fn: () => T) { fn = () => ({} as T); return { value: fn() }; } wrap(() => new C()).value.go();",
        "function wrap<T>(fn: () => T) { { const fn = () => ({}); return { value: fn() }; } } wrap(() => new C()).value.go();",
        "async function wrap<T>(fn: () => T) { return { value: fn() }; } wrap(() => new C()).value.go();",
        "function* wrap<T>(fn: () => T) { return { value: fn() }; } wrap(() => new C()).value.go();",
        "function wrap<T>(fn: () => T, second: () => unknown) { return { value: second() }; } wrap(() => new C(), () => ({})).value.go();",
        "function wrap<T>(fn: () => T) { return { value: fn() }; } let instance = new C(); const getter = () => instance; instance = { go() {} }; wrap(getter).value.go();",
        "function wrap<T>(fn: () => T) { return { value: fn() }; } wrap({ get: () => new C() }).value.go();",
    ] {
        assert_callback_projection(source, false);
    }
}

#[test]
fn vue_computed_laziness_and_setter_controls() {
    for source in [
        "import { computed } from 'vue'; const result = computed({ get: () => new C(), set: (_value: unknown) => {} }); result.value = { go() {} }; result.value.go();",
        "import { computed } from 'vue'; const callback = () => new C(); const alias = callback; computed(alias).value.go();",
        "import { computed } from 'vue'; computed({ get() { return new C(); }, set(_value: unknown) {} }).value.go();",
    ] {
        assert_callback_projection(source, true);
    }
    for source in [
        "import { computed } from 'vue'; let instance = new C(); const result = computed(() => instance); instance = { go() {} }; result.value.go();",
        "import { computed } from 'vue'; const result = computed({ get: () => ({ go() {} }), set: (_value: C) => {} }); result.value = new C(); result.value.go();",
        "import { computed } from 'vue'; computed(() => new C().direct()).value.go();",
        "import { computed } from 'vue'; computed(function* () { return new C(); }).value.go();",
    ] {
        assert_callback_projection(source, false);
    }
}

#[test]
fn callback_wrapper_assigned_value_keeps_producer_authority() {
    for (module, used) in [
        ("export { computed as wrap } from 'vue';", false),
        (
            "export function wrap(fn: () => unknown) { return { value: fn() }; }",
            true,
        ),
    ] {
        assert_callback_files(
            "import { wrap } from './helper'; const result = wrap(() => ({ go() {} })); result.value = new C(); const saved = result.value; saved.go();",
            used,
            &[("helper.ts", module)],
        );
    }
}

#[test]
fn callback_wrapper_return_declarations_must_match() {
    assert_callback_projection(
        "function wrap<T>(fn: () => T) { return { value: fn() }; } declare const flag: boolean; wrap(() => { if (flag) { class C { go() {} } return new C(); } return new C(); }).value.go();",
        false,
    );
    assert_callback_projection(
        "function wrap<T>(fn: () => T) { return { value: fn() }; } declare const flag: boolean; wrap(() => { if (flag) return new C(); return new C(); }).value.go();",
        true,
    );
}

#[test]
fn callback_wrapper_returned_object_binding_keeps_callback_source() {
    assert_callback_projection(
        "function wrap<T>(fn: () => T) { const result = { value: fn() }; return result; } wrap(() => new C()).value.go();",
        true,
    );
    assert_callback_projection(
        "function wrap<T>(fn: () => T) { const result = { value: fn() }; result.value = {} as T; return result; } wrap(() => new C()).value.go();",
        false,
    );
}

#[test]
fn callback_wrapper_cross_module_identity_controls() {
    for source in [
        "import * as helpers from './helper'; helpers.wrap(() => new C()).value.go();",
        "import { wrap } from './barrel'; const alias = wrap; alias<C>(() => new C()).value.go();",
    ] {
        assert_callback_files(
            source,
            true,
            &[
                (
                    "helper.ts",
                    "import { C } from './decoy'; export function wrap<T>(fn: () => T) { return { value: fn() }; } new C().direct();",
                ),
                ("barrel.ts", "export { wrap } from './helper';"),
                (
                    "decoy.ts",
                    "export class C { go() {} direct() {} dead() {} }",
                ),
            ],
        );
    }
    assert_callback_files(
        "import { wrap } from './helper'; wrap(() => new C()).value.go();",
        false,
        &[(
            "helper.ts",
            "export function wrap<T>(fn: () => T) { return { value: { go() {} } }; }",
        )],
    );
}

#[test]
fn vue_computed_alias_and_local_export_controls() {
    for source in [
        "import { computed } from 'vue'; const derive = computed; derive(() => new C()).value.go();",
        "import { derive } from './barrel'; derive(() => new C()).value.go();",
        "import { computed } from 'vue'; function getter() { return new C(); } computed(getter).value.go();",
    ] {
        assert_callback_files(
            source,
            true,
            &[(
                "barrel.ts",
                "import { computed } from '@vue/reactivity'; export { computed as derive };",
            )],
        );
    }
    assert_callback_projection(
        "import { computed } from 'vue'; let current = new C(); const result = computed({ get: () => current, set: (next: unknown) => { current = next; } }); result.value.go();",
        false,
    );
}

#[test]
fn retained_callback_wrapper_callers() {
    for fixture in [
        "class-callback-computed-value",
        "class-callback-computed-unbound-value",
        "class-callback-computed-explicit-generic",
        "class-callback-renamed-local-wrapper",
    ] {
        let mut config = create_config(super::common::fixture_path(fixture));
        config.rules.unused_class_members = fallow_config::Severity::Error;
        let result = fallow_core::analyze(&config).expect("retained caller analysis");
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
            "{fixture}: exact target class projection"
        );
    }
}

#[test]
fn review_r1_callback_body_write_invalidates_captured_value() {
    assert_callback_projection(
        "function wrap(fn) { return { value: fn() }; } let current = new C(); wrap(() => { current = { go() {} }; return current; }).value.go();",
        false,
    );
}

#[test]
fn review_r1_callback_alias_body_write_invalidates_captured_value() {
    assert_callback_projection(
        "function wrap(fn) { return { value: fn() }; } let current = new C(); const getter = () => { current = { go() {} }; return current; }; wrap(getter).value.go();",
        false,
    );
}

#[test]
fn review_r1_imported_callback_body_write_invalidates_captured_value() {
    assert_callback_files(
        "import { wrap } from './helper'; let current = new C(); wrap(() => { current = { go() {} }; return current; }).value.go();",
        false,
        &[(
            "helper.ts",
            "export function wrap(fn) { return { value: fn() }; }",
        )],
    );
}

#[test]
fn review_r2_spread_before_callback_has_unknown_ordinal() {
    assert_callback_projection(
        "function wrap(ignored, fn) { return { value: fn() }; } wrap(...[0, () => ({ go() {} })], () => new C()).value.go();",
        false,
    );
}

#[test]
fn review_r2_imported_spread_before_callback_has_unknown_ordinal() {
    assert_callback_files(
        "import { wrap } from './helper'; wrap(...[0, () => ({ go() {} })], () => new C()).value.go();",
        false,
        &[(
            "helper.ts",
            "export function wrap(ignored, fn) { return { value: fn() }; }",
        )],
    );
}

#[test]
fn review_r3_vue_getter_options_alias_preserves_value() {
    assert_callback_projection(
        "import { computed } from 'vue'; const options = { get: () => new C(), set: (value) => {} }; computed(options).value.go();",
        true,
    );
}

#[test]
fn review_preservation_callback_snapshots_and_trailing_spread() {
    for source in [
        "function wrap(fn) { return { value: fn() }; } let current = new C(); const result = wrap(() => current); current = { go() {} }; result.value.go();",
        "function wrap(fn) { return { value: fn() }; } let current = new C(); const getter = () => current; const result = wrap(getter); current = { go() {} }; result.value.go();",
        "function wrap(fn) { return { value: fn() }; } wrap(() => new C(), ...[]).value.go();",
    ] {
        assert_callback_projection(source, true);
    }
    assert_callback_files(
        "import { wrap } from './helper'; wrap(() => new C(), ...[]).value.go();",
        true,
        &[(
            "helper.ts",
            "export function wrap(fn) { return { value: fn() }; }",
        )],
    );
}

#[test]
fn review_r1_callback_conditional_write_and_shadow_controls() {
    assert_callback_projection(
        "function wrap(fn) { return { value: fn() }; } let current = new C(); wrap(() => { if (Date.now()) current = { go() {} }; return current; }).value.go();",
        false,
    );
    assert_callback_projection(
        "function wrap(fn) { return { value: fn() }; } let current = new C(); wrap(() => { { let current = {}; current = { go() {} }; } return current; }).value.go();",
        true,
    );
    assert_callback_files(
        "import { wrap } from './helper'; let current = new C(); const result = wrap(() => current); current = { go() {} }; result.value.go();",
        true,
        &[(
            "helper.ts",
            "export function wrap(fn) { return { value: fn() }; }",
        )],
    );
}

#[test]
fn review_r3_vue_getter_options_shared_alias_controls() {
    for source in [
        "import { computed } from 'vue'; const options = { get: () => new C(), set: value => {} }; const alias = options; const alias2 = alias; const result = computed(alias2); result.value = { go() {} }; result.value.go();",
        "import { computed } from 'vue'; const options = { get: () => new C(), set: value => {} }; const alias = options; const result = computed(alias); alias.get = () => ({ go() {} }); result.value.go();",
        "import { computed } from 'vue'; const options = { get() { return new C(); }, set(value) {} }; computed(options).value.go();",
        "import { computed } from 'vue'; const options = { get: () => new C(), set: value => {} }; const alias = options; alias.set = value => {}; computed(options).value.go();",
    ] {
        assert_callback_projection(source, true);
    }
}

#[test]
fn review_r3_vue_getter_options_mutation_controls() {
    for source in [
        "import { computed } from 'vue'; const options = { get: () => new C(), set: value => {} }; const alias = options; alias.get = () => ({ go() {} }); computed(options).value.go();",
        "import { computed } from 'vue'; let options = { get: () => new C(), set: value => {} }; options = { get: () => ({ go() {} }), set: value => {} }; computed(options).value.go();",
        "import { computed } from 'vue'; const options = { get: () => ({ go() {} }), set: value => {} }; const result = computed(options); result.value = new C(); result.value.go();",
        "import { computed } from 'vue'; let current = new C(); const options = { get: () => current, set: value => { current = value; } }; computed(options).value.go();",
        "import { computed } from 'vue'; const options = { get get() { return new C(); }, set: value => {} }; computed(options).value.go();",
    ] {
        assert_callback_projection(source, false);
    }
}

#[test]
fn review_second_r1_local_arrow_parameter_write() {
    assert_callback_files(
        "function wrap(fn) { return { value: fn() }; } let current = new C(); wrap((ignored = (current = { go() {} })) => current).value.go();",
        false,
        &[(
            "helper.ts",
            "export function wrap(fn) { return { value: fn() }; }",
        )],
    );
}

#[test]
fn review_second_r1_imported_arrow_parameter_write() {
    assert_callback_files(
        "import { wrap } from './helper'; let current = new C(); wrap((ignored = (current = { go() {} })) => current).value.go();",
        false,
        &[(
            "helper.ts",
            "export function wrap(fn) { return { value: fn() }; }",
        )],
    );
}

#[test]
fn review_second_r1_local_block_parameter_write() {
    assert_callback_files(
        "function wrap(fn) { return { value: fn() }; } let current = new C(); wrap(function (ignored = (current = { go() {} })) { return current; }).value.go();",
        false,
        &[(
            "helper.ts",
            "export function wrap(fn) { return { value: fn() }; }",
        )],
    );
}

#[test]
fn review_second_r1_imported_block_parameter_write() {
    assert_callback_files(
        "import { wrap } from './helper'; let current = new C(); wrap(function (ignored = (current = { go() {} })) { return current; }).value.go();",
        false,
        &[(
            "helper.ts",
            "export function wrap(fn) { return { value: fn() }; }",
        )],
    );
}

#[test]
fn review_second_r1_local_destructured_parameter_write() {
    assert_callback_files(
        "function wrap(fn) { return { value: fn() }; } let current = new C(); wrap(({ ignored = (current = { go() {} }) } = {}) => current).value.go();",
        false,
        &[(
            "helper.ts",
            "export function wrap(fn) { return { value: fn() }; }",
        )],
    );
}

#[test]
fn review_second_r1_imported_destructured_parameter_write() {
    assert_callback_files(
        "import { wrap } from './helper'; let current = new C(); wrap(({ ignored = (current = { go() {} }) } = {}) => current).value.go();",
        false,
        &[(
            "helper.ts",
            "export function wrap(fn) { return { value: fn() }; }",
        )],
    );
}

#[test]
fn review_second_r3_named_getter_in_options_preserves_value() {
    for getter in [
        "const getter = () => new C();",
        "function getter() { return new C(); }",
    ] {
        assert_callback_projection(
            &format!(
                "import {{ computed }} from 'vue'; {getter} const alias = getter; const options = {{ get: alias, set: value => {{}} }}; computed(options).value.go();"
            ),
            true,
        );
    }
}

#[test]
fn review_second_r1_parameter_and_snapshot_controls() {
    for callback in [
        "(ignored = 0) => current",
        "({ ignored = 0 } = {}) => current",
        "(ignored = (() => { let current = {}; current = {}; })()) => current",
    ] {
        for helper in [
            "function wrap(fn) { return { value: fn() }; }",
            "import { wrap } from './helper';",
        ] {
            assert_callback_files(
                &format!(
                    "{helper} let current = new C(); const result = wrap({callback}); current = {{ go() {{}} }}; result.value.go();"
                ),
                true,
                &[(
                    "helper.ts",
                    "export function wrap(fn) { return { value: fn() }; }",
                )],
            );
        }
    }
    assert_callback_projection(
        "function wrap(fn) { return { value: fn() }; } let current = new C(); function getter(ignored = (current = { go() {} })) { return current; } wrap(getter).value.go();",
        false,
    );
    assert_callback_projection(
        "import { computed } from 'vue'; let current = new C(); computed((ignored = 0) => current).value.go(); current = { go() {} };",
        false,
    );
}

#[test]
fn review_second_r3_named_getter_snapshot_controls() {
    for source in [
        "const getter = () => new C(); const options = { get: getter, set: value => {} }; const alias = options; const result = computed(alias); result.value = { go() {} }; result.value.go();",
        "let getter = () => new C(); const options = { get: getter, set: value => {} }; getter = () => ({ go() {} }); computed(options).value.go();",
        "const getter = () => new C(); const options = { get: getter }; const result = computed(options); delete options.get; result.value.go();",
    ] {
        assert_callback_projection(&format!("import {{ computed }} from 'vue'; {source}"), true);
    }
}

#[test]
fn review_second_r3_named_getter_uncertainty_controls() {
    for source in [
        "let getter = () => new C(); getter = () => ({ go() {} }); const options = { get: getter }; computed(options).value.go();",
        "const getter = () => new C(); const options = { get: getter }; const alias = options; delete alias.get; computed(options).value.go();",
        "const getter = () => new C(); const options = { get: getter }; const alias = options; alias.get = () => ({ go() {} }); computed(options).value.go();",
        "let current = new C(); const getter = () => current; const options = { get: getter, set: value => { current = value; } }; computed(options).value.go();",
        "const getter = () => new C(); const options = { get get() { return getter; } }; computed(options).value.go();",
        "const getter = () => new C(); { const getter = () => ({ go() {} }); const options = { get: getter }; computed(options).value.go(); }",
    ] {
        assert_callback_projection(
            &format!("import {{ computed }} from 'vue'; {source}"),
            false,
        );
    }
    assert_callback_projection(
        "import { computed } from 'foreign'; const getter = () => new C(); const options = { get: getter, set: value => {} }; computed(options).value.go();",
        false,
    );
}
