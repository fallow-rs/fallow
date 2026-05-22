use super::common::{create_config, create_config_with_ignore_decorators, fixture_path};

// ── Enum/class members integration ─────────────────────────────

#[test]
fn enum_class_members_detects_unused_members() {
    let root = fixture_path("enum-class-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_enum_member_names: Vec<&str> = results
        .unused_enum_members
        .iter()
        .map(|m| m.member.member_name.as_str())
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
        .map(|m| m.member.member_name.as_str())
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
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
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
        .map(|m| m.member.member_name.as_str())
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
        .map(|m| m.member.member_name.as_str())
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
        .map(|m| (m.member.parent_name.as_str(), m.member.member_name.as_str()))
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

#[test]
fn playwright_fixture_pom_methods_are_credited_from_tests() {
    let root = fixture_path("playwright-pom-fixtures");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused_class_members.contains(&"AdminPage.assertGreeting".to_string()),
        "AdminPage.assertGreeting should be credited through the typed Playwright fixture, found: {unused_class_members:?}"
    );
    assert!(
        !unused_class_members.contains(&"UserPage.assertGreeting".to_string()),
        "UserPage.assertGreeting should be credited through the typed Playwright fixture, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&"AdminPage.unusedAdminOnly".to_string()),
        "genuinely unused POM methods should still be reported, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&"UserPage.unusedUserOnly".to_string()),
        "genuinely unused POM methods should still be reported, found: {unused_class_members:?}"
    );
}

#[test]
fn playwright_nested_fixture_pom_methods_are_credited_from_tests() {
    let root = fixture_path("playwright-pom-fixtures-nested");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused_class_members.contains(&"AdminPage.assertGreeting".to_string()),
        "AdminPage.assertGreeting should be credited through nested-fixture chained access (pages.adminPage.assertGreeting), found: {unused_class_members:?}"
    );
    assert!(
        !unused_class_members.contains(&"UserPage.assertGreeting".to_string()),
        "UserPage.assertGreeting should be credited through nested-fixture destructuring ({{ pages: {{ userPage }} }}), found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&"AdminPage.unusedAdminOnly".to_string()),
        "genuinely unused POM methods should still be reported, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&"UserPage.unusedUserOnly".to_string()),
        "genuinely unused POM methods should still be reported, found: {unused_class_members:?}"
    );
}

#[test]
fn playwright_helper_function_fixture_pom_methods_are_credited() {
    let root = fixture_path("issue-491-playwright-fixture-helper-function");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused_class_members.contains(&"LoginActions.openLogin".to_string()),
        "LoginActions.openLogin should be credited through the helper-function fixture (appTest()()), found: {unused_class_members:?}"
    );
    assert!(
        !unused_class_members.contains(&"AdminActions.openAdmin".to_string()),
        "AdminActions.openAdmin should be credited through nested destructuring on the helper-function fixture, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&"LoginActions.unusedLoginOnly".to_string()),
        "genuinely unused POM methods should still be reported, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&"AdminActions.unusedAdminOnly".to_string()),
        "genuinely unused POM methods should still be reported, found: {unused_class_members:?}"
    );
}

#[test]
fn playwright_helper_function_with_local_setup_fixture_pom_methods_are_credited() {
    let root = fixture_path("issue-586-playwright-helper-local-setup");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused_class_members.contains(&"SidebarActionsLocal.openPatientsWithLocal".to_string()),
        "SidebarActionsLocal.openPatientsWithLocal should be credited through a helper that has local setup before returning base.extend, found: {unused_class_members:?}"
    );
    assert!(
        !unused_class_members.contains(&"SidebarActionsDirect.openPatientsDirect".to_string()),
        "SidebarActionsDirect.openPatientsDirect should remain credited through the direct-return control helper, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&"SidebarActionsLocal.unusedLocalOnly".to_string()),
        "genuinely unused local POM methods should still be reported, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&"SidebarActionsDirect.unusedDirectOnly".to_string()),
        "genuinely unused direct POM methods should still be reported, found: {unused_class_members:?}"
    );
}

#[test]
fn playwright_fixture_teardown_credits_factory_getter_member_usage() {
    let root = fixture_path("issue-386-playwright-fixture-teardown");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused_class_members.contains(&"ProcessEventsService.queryEventsForProcessId".to_string()),
        "ProcessEventsService.queryEventsForProcessId should be credited through a Playwright fixture teardown factory getter, found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&"ProcessEventsService.unusedServiceMethod".to_string()),
        "genuinely unused service methods should still be reported, found: {unused_class_members:?}"
    );
    assert!(
        !unused_class_members
            .iter()
            .any(|member| member.starts_with("AuditService.")),
        "Object.keys(factory.auditService) should credit the whole target service through the typed getter chain, found: {unused_class_members:?}"
    );
}

#[test]
fn fluent_builder_chain_credits_intermediate_setters() {
    let root = fixture_path("issue-387-fluent-builder");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    for credited in [
        "EventBuilder.setEventVersion",
        "EventBuilder.setProcessId",
        "EventBuilder.setSubject",
        "EventBuilder.build",
    ] {
        assert!(
            !unused_class_members.contains(&credited.to_string()),
            "{credited} is reached through a fluent-builder chain off `EventBuilder.createWithDefaults()` / `EventBuilder.create()`, should be credited (issue #387), found: {unused_class_members:?}"
        );
    }
    assert!(
        unused_class_members.contains(&"EventBuilder.setUnused".to_string()),
        "genuinely unused fluent setters should still be reported, found: {unused_class_members:?}"
    );
    // Negative case: `EventBuilder.format("x").trim()` emits a fluent-chain
    // sentinel, but `format` is NOT `is_instance_returning_static` (it returns
    // string). The analyze-layer `!has_factory` guard must reject the chain so
    // `fakeFromNonFactory` stays reported as unused.
    assert!(
        unused_class_members.contains(&"EventBuilder.fakeFromNonFactory".to_string()),
        "fluent-chain credit must not piggy-back on a non-factory root method, found: {unused_class_members:?}"
    );
}

#[test]
fn generic_constrained_param_credits_base_class_member() {
    let root = fixture_path("issue-388-generic-constraint");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused_class_members.contains(&"BaseClient.fetchLatest".to_string()),
        "BaseClient.fetchLatest is called via a generic-constrained `this.client`, should be credited (issue #388), found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&"BaseClient.unusedBaseMethod".to_string()),
        "genuinely unused base methods should still be reported, found: {unused_class_members:?}"
    );
}

#[test]
fn angular_inject_fields_credit_service_member_usage() {
    let root = fixture_path("angular-inject-class-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_class_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused_class_members.contains(&"InnerService.aaa".to_string()),
        "InnerService.aaa should be credited through this.inner.aaa where inner = inject(InnerService), found: {unused_class_members:?}"
    );
    assert!(
        !unused_class_members.contains(&"InnerService.bbb".to_string()),
        "InnerService.bbb should be credited through this.inner.bbb where inner = inject(InnerService), found: {unused_class_members:?}"
    );
    assert!(
        unused_class_members.contains(&"InnerService.ccc".to_string()),
        "InnerService.ccc should still be reported as genuinely unused, found: {unused_class_members:?}"
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
        .map(|m| m.member.member_name.as_str())
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
        .map(|m| m.member.member_name.as_str())
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
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
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

// ── ignoreDecorators (issue #471) ──────────────────────────────

#[test]
fn ignore_decorators_unlocks_only_listed_decorators() {
    let root = fixture_path("ignore-decorators-mixed");
    let config = create_config_with_ignore_decorators(root, vec!["@step".to_string()]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    // @step is in the ignore list; decoratedOnly has only @step, so it gets
    // checked normally and surfaces as unused.
    assert!(
        unused.contains(&"Demo.decoratedOnly".to_string()),
        "decoratedOnly carries only @step and should be reported, found: {unused:?}"
    );
    // plainUnused has no decorators; the existing detector reports it.
    assert!(
        unused.contains(&"Demo.plainUnused".to_string()),
        "plainUnused has no decorators and should be reported, found: {unused:?}"
    );
    // mixed has @step AND @Inject; @Inject is not in the ignore list, so the
    // conservative skip still applies.
    assert!(
        !unused.contains(&"Demo.mixed".to_string()),
        "mixed carries a non-ignored @Inject and must stay skipped, found: {unused:?}"
    );
    // frameworkOnly has only @Inject (not ignored); stays skipped.
    assert!(
        !unused.contains(&"Demo.frameworkOnly".to_string()),
        "frameworkOnly carries only the non-ignored @Inject and must stay skipped, found: {unused:?}"
    );
    // actuallyUsed is called from entry.ts; never surfaces as unused regardless.
    assert!(
        !unused.contains(&"Demo.actuallyUsed".to_string()),
        "actuallyUsed is called from entry and must not be reported, found: {unused:?}"
    );
}

#[test]
fn ignore_decorators_dotted_entry_matches_exact_path() {
    let root = fixture_path("ignore-decorators-namespaced");
    let config = create_config_with_ignore_decorators(root, vec!["decorators.log".to_string()]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    // @decorators.log matches the dotted entry; loggedMethod is checked.
    assert!(
        unused.contains(&"Demo.loggedMethod".to_string()),
        "loggedMethod's @decorators.log matches the dotted entry and the method should be reported, found: {unused:?}"
    );
    // @decorators.audit does NOT match the dotted entry; auditedMethod stays skipped.
    assert!(
        !unused.contains(&"Demo.auditedMethod".to_string()),
        "auditedMethod's @decorators.audit is not in the ignore list and must stay skipped, found: {unused:?}"
    );
    // plainMethod has no decorators; reported as unused normally.
    assert!(
        unused.contains(&"Demo.plainMethod".to_string()),
        "plainMethod has no decorators and should be reported, found: {unused:?}"
    );
}

#[test]
fn ignore_decorators_bare_entry_collapses_namespace() {
    let root = fixture_path("ignore-decorators-namespaced");
    let config = create_config_with_ignore_decorators(root, vec!["decorators".to_string()]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    // Bare "decorators" matches the leftmost segment of every @decorators.*
    // path, so both decorated methods are checked.
    assert!(
        unused.contains(&"Demo.loggedMethod".to_string()),
        "loggedMethod's @decorators.log should match bare entry 'decorators', found: {unused:?}"
    );
    assert!(
        unused.contains(&"Demo.auditedMethod".to_string()),
        "auditedMethod's @decorators.audit should match bare entry 'decorators', found: {unused:?}"
    );
}

#[test]
fn ignore_decorators_applies_to_declaring_class_only() {
    // class Page { @step run() {} }; class AdminPage extends Page {}.
    // entry.ts references AdminPage only. With @step in the ignore list, the
    // declaring class's @step-decorated method is checked: Page.run should
    // surface as unused. AdminPage has no own members; the gate has nothing to
    // do with the child.
    let root = fixture_path("ignore-decorators-inheritance");
    let config = create_config_with_ignore_decorators(root, vec!["@step".to_string()]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        unused.contains(&"Page.run".to_string()),
        "Page.run carries only @step and should be reported on the declaring class, found: {unused:?}"
    );
    // AdminPage has zero members of its own; the gate is irrelevant for it.
    let admin_findings: Vec<&String> = unused
        .iter()
        .filter(|entry| entry.starts_with("AdminPage."))
        .collect();
    assert!(
        admin_findings.is_empty(),
        "AdminPage has no own members; no findings should be attributed to it, found: {admin_findings:?}"
    );
}
