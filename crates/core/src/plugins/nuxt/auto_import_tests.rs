//! Tests of the Nuxt auto-import directory walks.

use super::*;

#[test]
fn auto_imports_name_global_and_island_components_after_their_own_directory() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("components/global")).unwrap();
    std::fs::create_dir_all(root.join("components/islands")).unwrap();
    std::fs::create_dir_all(root.join("components/base")).unwrap();
    std::fs::write(
        root.join("components/global/Foo.vue"),
        "<template></template>",
    )
    .unwrap();
    std::fs::write(
        root.join("components/islands/Isle.vue"),
        "<template></template>",
    )
    .unwrap();
    std::fs::write(
        root.join("components/base/Button.vue"),
        "<template></template>",
    )
    .unwrap();

    let rules = NuxtPlugin.auto_imports(root);
    let names: std::collections::BTreeSet<&str> = rules.iter().map(|r| r.name.as_str()).collect();

    assert!(names.contains("Foo"), "global component keeps its own name");
    assert!(names.contains("LazyFoo"));
    assert!(names.contains("Isle"), "island keeps its own name");
    assert!(names.contains("BaseButton"), "other dirs still prefix");
    assert!(!names.contains("GlobalFoo"));
    assert!(!names.contains("IslandsIsle"));
}

#[test]
fn auto_imports_prefix_a_nested_global_directory_like_any_other() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("components/base/global")).unwrap();
    std::fs::write(
        root.join("components/base/global/Card.vue"),
        "<template></template>",
    )
    .unwrap();

    let names: std::collections::BTreeSet<String> = NuxtPlugin
        .auto_imports(root)
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert!(names.contains("BaseGlobalCard"));
    assert!(!names.contains("Card"));
}

#[test]
fn auto_imports_emits_canonical_and_lazy_variants() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("components/base")).unwrap();
    std::fs::write(root.join("components/Card001.vue"), "<template></template>").unwrap();
    std::fs::write(
        root.join("components/base/Button.vue"),
        "<template></template>",
    )
    .unwrap();

    let rules = NuxtPlugin.auto_imports(root);
    let names: std::collections::BTreeSet<&str> = rules.iter().map(|r| r.name.as_str()).collect();

    assert!(names.contains("Card001"));
    assert!(names.contains("LazyCard001"));
    assert!(names.contains("BaseButton"));
    assert!(names.contains("LazyBaseButton"));
    assert!(
        rules
            .iter()
            .all(|r| matches!(r.kind, AutoImportKind::DefaultComponent)),
        "component rules are DefaultComponent kind"
    );
    let card = rules.iter().find(|r| r.name == "Card001").unwrap();
    assert_eq!(card.source, root.join("components/Card001.vue"));
}

#[test]
fn auto_imports_paired_client_server_share_a_name_with_distinct_sources() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("app/components")).unwrap();
    std::fs::write(
        root.join("app/components/Comments.client.vue"),
        "<template></template>",
    )
    .unwrap();
    std::fs::write(
        root.join("app/components/Comments.server.vue"),
        "<template></template>",
    )
    .unwrap();

    let rules = NuxtPlugin.auto_imports(root);
    let comments_sources: std::collections::BTreeSet<_> = rules
        .iter()
        .filter(|r| r.name == "Comments")
        .map(|r| r.source.clone())
        .collect();
    assert_eq!(comments_sources.len(), 2);
}

#[test]
fn auto_imports_emits_script_named_and_default_exports() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("composables")).unwrap();
    std::fs::create_dir_all(root.join("utils")).unwrap();
    std::fs::write(
        root.join("composables/useCounter.ts"),
        "export function useCounter() { return 1; }\nexport type CounterType = number;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("utils/format-price.ts"),
        "export default function format(value: number) { return String(value); }\n",
    )
    .unwrap();

    let rules = NuxtPlugin.auto_imports(root);
    let use_counter = rules
        .iter()
        .find(|rule| rule.name == "useCounter")
        .expect("named export rule");
    assert_eq!(use_counter.source, root.join("composables/useCounter.ts"));
    assert!(matches!(use_counter.kind, AutoImportKind::Named));
    assert!(
        rules.iter().all(|rule| rule.name != "CounterType"),
        "type-only exports are not value auto-import rules"
    );
    let format_price = rules
        .iter()
        .find(|rule| rule.name == "formatPrice")
        .expect("default export rule");
    assert_eq!(format_price.source, root.join("utils/format-price.ts"));
    assert!(matches!(format_price.kind, AutoImportKind::Default));
}

#[test]
fn auto_imports_scans_shared_recursively() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("shared/utils/nested")).unwrap();
    std::fs::write(
        root.join("shared/utils/useShared.ts"),
        "export const useShared = () => null;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("shared/utils/nested/useDeep.ts"),
        "export const useDeep = () => null;\n",
    )
    .unwrap();

    let rules = NuxtPlugin.auto_imports(root);
    assert!(rules.iter().any(|rule| rule.name == "useShared"));
    assert!(
        rules.iter().any(|rule| rule.name == "useDeep"),
        "nested shared utils are scanned by default"
    );
}

#[test]
fn auto_imports_empty_without_convention_dirs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    assert!(NuxtPlugin.auto_imports(tmp.path()).is_empty());
}

/// A Nuxt config, a composable file and a components directory that link
/// out of the project are not read.
#[test]
#[cfg(unix)]
fn nuxt_files_that_link_out_of_the_project_are_not_read() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("project");
    let outside = tmp.path().join("elsewhere");
    std::fs::create_dir_all(root.join("composables")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(
        outside.join("nuxt.config.ts"),
        "export default defineNuxtConfig({\n  components: [{ path: '~/ui', prefix: 'U' }],\n})\n",
    )
    .unwrap();
    std::fs::write(
        outside.join("useOutside.ts"),
        "export const useOutside = () => null;\n",
    )
    .unwrap();
    std::fs::write(outside.join("Outside.vue"), "<template />\n").unwrap();
    std::os::unix::fs::symlink(outside.join("nuxt.config.ts"), root.join("nuxt.config.ts"))
        .unwrap();
    std::os::unix::fs::symlink(
        outside.join("useOutside.ts"),
        root.join("composables/useOutside.ts"),
    )
    .unwrap();
    std::os::unix::fs::symlink(&outside, root.join("components")).unwrap();

    std::fs::create_dir_all(root.join("layers/base")).unwrap();
    std::fs::write(
        outside.join("layer.config.ts"),
        "export default defineNuxtConfig({ $meta: { name: 'outside' } })\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        outside.join("layer.config.ts"),
        root.join("layers/base/nuxt.config.ts"),
    )
    .unwrap();
    assert_eq!(layer_name(&root, "layers/base", false), None);
    let settings = auto_import_settings(&root);
    assert_eq!(settings.components, AutoImportSetting::Default);
    let rules = NuxtPlugin.auto_imports(&root);
    assert!(!rules.iter().any(|rule| rule.name == "useOutside"));
    assert!(!rules.iter().any(|rule| rule.name == "Outside"));
}

/// A layer out of the read scope of the project is not probed, so it is
/// never a layer root. A layer in the same workspace stays one.
#[test]
fn outside_layer_roots_skip_a_layer_out_of_the_project_scope() {
    let sandbox = tempfile::tempdir().expect("tempdir");
    let app = sandbox.path().join("app");
    let ui = sandbox.path().join("ui");
    std::fs::create_dir_all(&app).expect("create app");
    std::fs::create_dir_all(&ui).expect("create ui");
    std::fs::write(
        app.join("nuxt.config.ts"),
        "export default defineNuxtConfig({ extends: ['../ui'] })",
    )
    .expect("write app config");
    std::fs::write(
        ui.join("nuxt.config.ts"),
        "export default defineNuxtConfig({})",
    )
    .expect("write layer config");
    assert!(outside_layer_roots(&app).is_empty());

    fallow_config::clear_project_scope_bases();
    std::fs::write(
        sandbox.path().join("pnpm-workspace.yaml"),
        "packages:\n  - app\n  - ui\n",
    )
    .expect("write workspace");
    assert_eq!(outside_layer_roots(&app), [ui]);
    fallow_config::clear_project_scope_bases();
}
