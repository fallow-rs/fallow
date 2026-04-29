use super::common::{create_config, fixture_path};

// ── Enum/class members integration ─────────────────────────────

#[test]
fn enum_class_members_detects_unused_members() {
    let root = fixture_path("enum-class-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_enum_member_names: Vec<&str> = results
        .unused_enum_members
        .iter()
        .map(|m| m.member_name.as_str())
        .collect();

    // Only Status.Active is used; Inactive and Pending should be unused
    assert!(
        unused_enum_member_names.contains(&"Inactive"),
        "Inactive should be detected as unused enum member, found: {unused_enum_member_names:?}"
    );
    assert!(
        unused_enum_member_names.contains(&"Pending"),
        "Pending should be detected as unused enum member, found: {unused_enum_member_names:?}"
    );

    let unused_class_member_names: Vec<&str> = results
        .unused_class_members
        .iter()
        .map(|m| m.member_name.as_str())
        .collect();

    // unusedMethod is never called
    assert!(
        unused_class_member_names.contains(&"unusedMethod"),
        "unusedMethod should be detected as unused class member, found: {unused_class_member_names:?}"
    );

    // greet() is called via instance: `const svc = new MyService(); svc.greet()`
    assert!(
        !unused_class_member_names.contains(&"greet"),
        "greet should NOT be unused (called via instance), found: {unused_class_member_names:?}"
    );

    // name property is never accessed (not via svc.name or this.name)
    assert!(
        unused_class_member_names.contains(&"name"),
        "name should be detected as unused class property, found: {unused_class_member_names:?}"
    );
}

#[test]
fn exported_instance_class_members_are_credited_to_class() {
    let root = fixture_path("exported-instance-class-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.parent_name, m.member_name))
        .collect();

    assert!(
        !unused_class_members.contains(&"Box.bump".to_string()),
        "Box.bump should be credited through exported instance usage, found: {unused_class_members:?}"
    );
    assert!(
        !unused_class_members.contains(&"Box.current".to_string()),
        "Box.current getter/setter should be credited through exported instance usage, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&"Box.unused".to_string()),
        "Box.unused should still be reported, found: {unused_class_members:?}"
    );
}

// ── Cross-package enum/class member access (issue #178) ────────

#[test]
fn cross_package_enum_class_members_credit_re_exported_origin() {
    let root = fixture_path("cross-package-enum-class-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_enum_member_names: Vec<&str> = results
        .unused_enum_members
        .iter()
        .map(|m| m.member_name.as_str())
        .collect();

    // StatusCode.Active/Inactive/Pending are referenced cross-package via
    // `import { StatusCode } from '@repro/lib-a'` then `StatusCode.Active`,
    // where the `@repro/lib-a` import resolves to the barrel `index.ts`.
    // Without re-export chain propagation in `find_unused_members`, all
    // four members would be flagged. After the fix, only the genuinely
    // unused `Archived` should be reported.
    assert!(
        !unused_enum_member_names.contains(&"Active"),
        "StatusCode.Active should be credited via cross-package access, found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Inactive"),
        "StatusCode.Inactive should be credited via cross-package access, found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Pending"),
        "StatusCode.Pending should be credited via cross-package access, found: {unused_enum_member_names:?}"
    );
    assert!(
        unused_enum_member_names.contains(&"Archived"),
        "StatusCode.Archived is genuinely unused and should still be flagged, found: {unused_enum_member_names:?}"
    );

    // Direction: only East and West are referenced cross-package.
    assert!(
        !unused_enum_member_names.contains(&"East"),
        "Direction.East should be credited via cross-package access, found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"West"),
        "Direction.West should be credited via cross-package access, found: {unused_enum_member_names:?}"
    );
    assert!(
        unused_enum_member_names.contains(&"North"),
        "Direction.North is genuinely unused, found: {unused_enum_member_names:?}"
    );
    assert!(
        unused_enum_member_names.contains(&"South"),
        "Direction.South is genuinely unused, found: {unused_enum_member_names:?}"
    );

    // Class static method case from the issue comment: StringUtils.toUpper
    // is called cross-package; the other two static methods are not.
    let unused_class_member_names: Vec<&str> = results
        .unused_class_members
        .iter()
        .map(|m| m.member_name.as_str())
        .collect();

    assert!(
        !unused_class_member_names.contains(&"toUpper"),
        "StringUtils.toUpper should be credited via cross-package access, found: {unused_class_member_names:?}"
    );
    assert!(
        unused_class_member_names.contains(&"toLower"),
        "StringUtils.toLower is genuinely unused, found: {unused_class_member_names:?}"
    );
    assert!(
        unused_class_member_names.contains(&"reverse"),
        "StringUtils.reverse is genuinely unused, found: {unused_class_member_names:?}"
    );
}

#[test]
fn injected_dependency_object_credits_class_member_usage() {
    let root = fixture_path("injected-dependency-class-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<(&str, &str)> = results
        .unused_class_members
        .iter()
        .map(|m| (m.parent_name.as_str(), m.member_name.as_str()))
        .collect();

    assert!(
        !unused_class_members.contains(&("FooClass", "foo")),
        "FooClass.foo should be credited through this.deps.foo.foo(), found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&("FooClass", "unused")),
        "the fixture should still report genuinely unused members, found: {unused_class_members:?}"
    );
}

// ── Whole-object enum member heuristics ────────────────────────

#[test]
fn enum_whole_object_uses_no_false_positives() {
    let root = fixture_path("enum-whole-object");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_enum_member_names: Vec<&str> = results
        .unused_enum_members
        .iter()
        .map(|m| m.member_name.as_str())
        .collect();

    // Status used via Object.values — no members should be unused
    assert!(
        !unused_enum_member_names.contains(&"Active"),
        "Active should not be unused (Object.values), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Inactive"),
        "Inactive should not be unused (Object.values), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Pending"),
        "Pending should not be unused (Object.values), found: {unused_enum_member_names:?}"
    );

    // Direction used via Object.keys — no members should be unused
    assert!(
        !unused_enum_member_names.contains(&"Up"),
        "Up should not be unused (Object.keys), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Down"),
        "Down should not be unused (Object.keys), found: {unused_enum_member_names:?}"
    );

    // Color used via for..in — no members should be unused
    assert!(
        !unused_enum_member_names.contains(&"Red"),
        "Red should not be unused (for..in), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Green"),
        "Green should not be unused (for..in), found: {unused_enum_member_names:?}"
    );

    // Priority — only High accessed via computed literal, Low and Medium should be unused
    assert!(
        unused_enum_member_names.contains(&"Low"),
        "Low should be unused (only High accessed via computed), found: {unused_enum_member_names:?}"
    );
    assert!(
        unused_enum_member_names.contains(&"Medium"),
        "Medium should be unused (only High accessed via computed), found: {unused_enum_member_names:?}"
    );
}

// ── Type-level enum member usage ──────────────────────────────

#[test]
fn enum_type_level_usage_no_false_positives() {
    let root = fixture_path("enum-type-level");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_enum_member_names: Vec<&str> = results
        .unused_enum_members
        .iter()
        .map(|m| m.member_name.as_str())
        .collect();

    // BreakpointString used as mapped type constraint — all members should be used
    assert!(
        !unused_enum_member_names.contains(&"xs"),
        "xs should not be unused (mapped type constraint), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"xxl"),
        "xxl should not be unused (mapped type constraint), found: {unused_enum_member_names:?}"
    );

    // Status.Active used via qualified type name, Status.Inactive via runtime access
    assert!(
        !unused_enum_member_names.contains(&"Active"),
        "Active should not be unused (type qualified name), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Inactive"),
        "Inactive should not be unused (runtime access), found: {unused_enum_member_names:?}"
    );

    // Status.Pending is not used in any way — should be unused
    assert!(
        unused_enum_member_names.contains(&"Pending"),
        "Pending should be unused (no type-level or runtime access), found: {unused_enum_member_names:?}"
    );

    // Color used via Record<Color, string> — all members should be used
    assert!(
        !unused_enum_member_names.contains(&"Red"),
        "Red should not be unused (Record<Color, T>), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Blue"),
        "Blue should not be unused (Record<Color, T>), found: {unused_enum_member_names:?}"
    );

    // Direction used via { [K in keyof typeof Direction]: ... } — all members should be used
    assert!(
        !unused_enum_member_names.contains(&"Up"),
        "Up should not be unused (keyof typeof in mapped type), found: {unused_enum_member_names:?}"
    );
    assert!(
        !unused_enum_member_names.contains(&"Right"),
        "Right should not be unused (keyof typeof in mapped type), found: {unused_enum_member_names:?}"
    );
}

// ── Typed-binding nullable unions ─────────

#[test]
fn typed_binding_through_nullable_unions_credits_class_methods() {
    let root = fixture_path("typed-binding-wrappers");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.parent_name, m.member_name))
        .collect();

    // `let pending: Aggregate | undefined; pending.rename();` reaches rename
    // through the nullable-union branch of `extract_type_reference_name`.
    assert!(
        !unused.contains(&"Aggregate.rename".to_string()),
        "Aggregate.rename should be credited through `Aggregate | undefined`, found unused: {unused:?}"
    );

    // `const ready: Promise<Aggregate> = ...; ready.archive();` is a member
    // access on the Promise object, not on Aggregate. It should not credit
    // Aggregate.archive.
    assert!(
        unused.contains(&"Aggregate.archive".to_string()),
        "Aggregate.archive should not be credited through `Promise<Aggregate>`, found unused: {unused:?}"
    );

    // unusedMethod has no call site in any form and should still be reported.
    assert!(
        unused.contains(&"Aggregate.unusedMethod".to_string()),
        "Aggregate.unusedMethod should still be flagged as unused, found unused: {unused:?}"
    );
}
