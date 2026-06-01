use super::common::{create_config, fixture_path};

#[test]
fn vue_project_discovers_vue_files() {
    let root = fixture_path("vue-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert!(
        !unused_file_names.contains(&"App.vue".to_string()),
        "App.vue should be reachable via import from main.ts, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"FancyCard.vue".to_string()),
        "FancyCard.vue is only used via a Vue component tag and should stay reachable: {unused_file_names:?}"
    );

    assert!(
        unused_file_names.contains(&"Orphan.vue".to_string()),
        "Orphan.vue should be detected as unused file, found: {unused_file_names:?}"
    );
}

#[test]
fn vue_imports_mark_exports_used() {
    let root = fixture_path("vue-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"formatDate"),
        "formatDate should be used from the Vue template, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"vFocusTrap"),
        "vFocusTrap should be used from a Vue template directive, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"handlers"),
        "handlers should be used from Vue v-on object syntax, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"dynamicAttr"),
        "dynamicAttr should be used from a Vue dynamic v-bind argument, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"dynamicEvent"),
        "dynamicEvent should be used from a Vue dynamic v-on argument, found: {unused_export_names:?}"
    );

    assert!(
        unused_export_names.contains(&"unusedUtil"),
        "unusedUtil should be detected as unused export, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"unusedImported"),
        "unusedImported should stay unused even when imported in App.vue, found: {unused_export_names:?}"
    );
}

#[test]
fn vue_template_event_handlers_mark_class_members_used() {
    let root = fixture_path("vue-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|member| {
            format!(
                "{}.{}",
                member.member.parent_name, member.member.member_name
            )
        })
        .collect();

    assert!(
        !unused_members.contains(&"Counter.bump".to_string()),
        "Counter.bump should be used from a Vue @click handler, found: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"Counter.value".to_string()),
        "Counter.value should be used from a Vue mustache expression, found: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"Counter.unused".to_string()),
        "Counter.unused should still be reported as unused, found: {unused_members:?}"
    );
}

#[test]
fn vue_component_tags_mark_component_exports_used() {
    let root = fixture_path("vue-component-tags");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                e.export.export_name.clone(),
            )
        })
        .collect();

    assert!(
        !unused_exports
            .iter()
            .any(|(file, export)| file == "GreetingCard.vue" && export == "default"),
        "GreetingCard default export should be used via component tags: {unused_exports:?}"
    );
    assert!(
        unused_exports
            .iter()
            .any(|(file, export)| file == "GreetingCard.vue" && export == "unusedNamed"),
        "GreetingCard named dead export should still be reported: {unused_exports:?}"
    );
}

#[test]
fn vue_template_edge_cases_mark_exports_used() {
    let root = fixture_path("vue-template-edges");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                e.export.export_name.clone(),
            )
        })
        .collect();

    for (file, export) in [
        ("utils.ts", "activeAttribute"),
        ("utils.ts", "attributeSources"),
        ("utils.ts", "fallbackItem"),
        ("utils.ts", "message"),
        ("utils.ts", "placement"),
        ("utils.ts", "unusedImported"),
        ("directives.ts", "vTooltip"),
    ] {
        assert!(
            !unused_exports
                .iter()
                .any(|(unused_file, unused_export)| unused_file == file && unused_export == export),
            "{file}:{export} should be preserved by Vue template usage, found: {unused_exports:?}"
        );
    }

    for (file, export) in [
        ("utils.ts", "unusedTemplateEdge"),
        ("directives.ts", "unusedDirectiveHelper"),
    ] {
        assert!(
            unused_exports
                .iter()
                .any(|(unused_file, unused_export)| unused_file == file && unused_export == export),
            "{file}:{export} should still be reported as unused, found: {unused_exports:?}"
        );
    }
}

#[test]
fn vue_split_value_type_exports_are_tracked_across_script_setup_usage() {
    let root = fixture_path("vue-split-type-value-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_types: Vec<(String, String)> = results
        .unused_types
        .iter()
        .map(|e| {
            (
                e.export
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                e.export.export_name.clone(),
            )
        })
        .collect();

    assert!(
        !unused_types
            .iter()
            .any(|(file, export)| file == "status.ts" && export == "Status"),
        "Status type export should be preserved by Vue script-setup type usage: {unused_types:?}"
    );
    assert!(
        unused_types
            .iter()
            .any(|(file, export)| file == "status.ts" && export == "UnusedStatus"),
        "UnusedStatus should still be reported: {unused_types:?}"
    );
}

#[test]
fn svelte_project_discovers_svelte_files() {
    let root = fixture_path("svelte-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert!(
        !unused_file_names.contains(&"App.svelte".to_string()),
        "App.svelte should be reachable via import from main.ts, unused: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"FancyButton.svelte".to_string()),
        "FancyButton.svelte is only used via a Svelte component tag and should stay reachable: {unused_file_names:?}"
    );

    assert!(
        unused_file_names.contains(&"Orphan.svelte".to_string()),
        "Orphan.svelte should be detected as unused file, found: {unused_file_names:?}"
    );
}

#[test]
fn svelte_imports_mark_exports_used() {
    let root = fixture_path("svelte-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"formatName"),
        "formatName should be used from the Svelte template, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"tooltip"),
        "tooltip should be used from a Svelte directive name, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"isActive"),
        "isActive should be used from a Svelte attribute value expression, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"myAttach"),
        "myAttach should be used from a Svelte {{@attach}} directive, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"inTernary"),
        "inTernary should be used from a Svelte ternary expression, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"inCallback"),
        "inCallback should be used from a Svelte method-chain callback reference, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"inSpread"),
        "inSpread should be used from a Svelte inline spread object, found: {unused_export_names:?}"
    );

    assert!(
        unused_export_names.contains(&"unusedUtil"),
        "unusedUtil should be detected as unused export, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"unusedImported"),
        "unusedImported should stay unused even when imported in App.svelte, found: {unused_export_names:?}"
    );
}

#[test]
fn svelte_template_event_handlers_mark_class_members_used() {
    let root = fixture_path("svelte-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|member| {
            format!(
                "{}.{}",
                member.member.parent_name, member.member.member_name
            )
        })
        .collect();

    assert!(
        !unused_members.contains(&"Counter.bump".to_string()),
        "Counter.bump should be used from a Svelte event handler arrow function, found: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"Counter.value".to_string()),
        "Counter.value should be used from a Svelte template expression, found: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"Counter.unused".to_string()),
        "Counter.unused should still be reported as unused, found: {unused_members:?}"
    );
}

#[test]
fn svelte_type_only_import_keeps_upstream_type_used() {
    let root = fixture_path("svelte-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_types: Vec<(String, &str)> = results
        .unused_types
        .iter()
        .map(|t| {
            let file = t
                .export
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            (file, t.export.export_name.as_str())
        })
        .collect();

    assert!(
        !unused_types
            .iter()
            .any(|(file, export)| file == "types.ts" && *export == "Greeting"),
        "Greeting must stay used: it backs `import type` + `: Greeting` annotation in App.svelte, got: {unused_types:?}"
    );
    assert!(
        unused_types
            .iter()
            .any(|(file, export)| file == "types.ts" && *export == "UnusedGreeting"),
        "UnusedGreeting must still be reported (sanity check that the fixture is wired): got: {unused_types:?}"
    );
}

#[test]
fn sveltekit_virtual_modules_not_unlisted() {
    let root = fixture_path("sveltekit-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        !unlisted_names.contains(&"$app"),
        "$app should not be unlisted (virtual module), found: {unlisted_names:?}"
    );
    assert!(
        !unlisted_names.contains(&"$env"),
        "$env should not be unlisted (virtual module), found: {unlisted_names:?}"
    );
}

#[test]
fn sveltekit_generated_types_not_unresolved() {
    let root = fixture_path("sveltekit-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();

    assert!(
        !unresolved_specs.contains(&"./$types"),
        "./$types should not be unresolved (generated import), found: {unresolved_specs:?}"
    );
    assert!(
        !unresolved_specs.contains(&"./$types.js"),
        "./$types.js should not be unresolved (generated import), found: {unresolved_specs:?}"
    );
}

#[test]
fn sveltekit_head_script_src_not_unresolved() {
    let root = fixture_path("issue-835-svelte-script-src");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();

    assert!(
        !unresolved_specs.contains(&"/some-lib.min.js"),
        "SvelteKit markup script src should not be unresolved: {unresolved_specs:?}"
    );
}

#[test]
fn sveltekit_workspace_types_not_unresolved() {
    let root = fixture_path("workspace-sveltekit");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();

    assert!(
        !unresolved_specs.contains(&"./$types"),
        "./$types should not be unresolved in workspace mode, found: {unresolved_specs:?}"
    );
}

#[test]
fn sveltekit_param_matchers_keep_match_export_alive() {
    let root = fixture_path("sveltekit-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                e.export.export_name.clone(),
            )
        })
        .collect();

    assert!(
        !unused_exports
            .iter()
            .any(|(file, export)| file == "integer.ts" && export == "match"),
        "SvelteKit param matcher export should be framework-used: {unused_exports:?}"
    );
    assert!(
        unused_exports
            .iter()
            .any(|(file, export)| file == "integer.ts" && export == "unusedParamHelper"),
        "SvelteKit matcher file should still report truly unused exports: {unused_exports:?}"
    );
}

#[test]
fn sveltekit_layout_reset_routes_are_entry_points() {
    let root = fixture_path("sveltekit-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused = unused_file_names(&results);

    assert!(
        !unused.contains(&"+page@.svelte".to_string()),
        "layout-reset page (`+page@.svelte`) should be an entry point: {unused:?}"
    );
    assert!(
        !unused.contains(&"+layout@named.svelte".to_string()),
        "layout-reset layout (`+layout@named.svelte`) should be an entry point: {unused:?}"
    );
    assert!(
        !unused.contains(&"+page@(checkout).svelte".to_string()),
        "group-form layout-reset page (`+page@(checkout).svelte`) should be an entry point: {unused:?}"
    );
}

fn unused_file_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect()
}

fn unused_export_pairs(results: &fallow_core::results::AnalysisResults) -> Vec<(String, String)> {
    results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                e.export.export_name.clone(),
            )
        })
        .collect()
}

#[test]
fn sveltekit_remote_files_are_reachable() {
    let root = fixture_path("sveltekit-remote-functions");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let files = unused_file_names(&results);

    assert!(
        !files.contains(&"posts.remote.ts".to_string()),
        "generated-binding-only remote file must not be unused: {files:?}"
    );
    assert!(
        !files.contains(&"data.remote.ts".to_string()),
        "imported remote file must not be unused: {files:?}"
    );
    assert!(
        files.contains(&"orphan.ts".to_string()),
        "non-remote orphan file must still report as unused: {files:?}"
    );
}

#[test]
fn sveltekit_remote_exports_are_credited() {
    let root = fixture_path("sveltekit-remote-functions");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let exports = unused_export_pairs(&results);

    for (file, name) in [
        ("data.remote.ts", "getData"),
        ("data.remote.ts", "submitData"),
        ("posts.remote.ts", "getPosts"),
        ("posts.remote.ts", "addPost"),
    ] {
        assert!(
            !exports.iter().any(|(f, e)| f == file && e == name),
            "remote function {file}#{name} must not report as unused export: {exports:?}"
        );
    }
}

#[test]
fn sveltekit_remote_exports_credited_under_include_entry_exports() {
    let root = fixture_path("sveltekit-remote-functions");
    let mut config = create_config(root);
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let exports = unused_export_pairs(&results);

    for (file, name) in [
        ("data.remote.ts", "getData"),
        ("data.remote.ts", "submitData"),
        ("posts.remote.ts", "getPosts"),
        ("posts.remote.ts", "addPost"),
    ] {
        assert!(
            !exports.iter().any(|(f, e)| f == file && e == name),
            "remote function {file}#{name} must stay credited under --include-entry-exports: {exports:?}"
        );
    }
}
