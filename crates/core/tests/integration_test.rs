#![expect(
    deprecated,
    reason = "ADR-008: integration tests exercise the workspace path-dep fallow_core::analyze* surface; the deprecation warning targets external crates.io consumers"
)]

#[path = "integration_test/common.rs"]
mod common;

#[path = "integration_test/angular_ng_package.rs"]
mod angular_ng_package;
#[path = "integration_test/barrel_exports.rs"]
mod barrel_exports;
#[path = "integration_test/basic_analysis.rs"]
mod basic_analysis;
#[path = "integration_test/caching.rs"]
mod caching;
#[path = "integration_test/css_modules.rs"]
mod css_modules;
#[path = "integration_test/dependencies.rs"]
mod dependencies;
#[path = "integration_test/duplicates.rs"]
mod duplicates;
#[path = "integration_test/dynamic_import_then.rs"]
mod dynamic_import_then;
#[path = "integration_test/dynamic_imports.rs"]
mod dynamic_imports;
#[path = "integration_test/external_plugins.rs"]
mod external_plugins;
#[path = "integration_test/extraction.rs"]
mod extraction;
#[path = "integration_test/false_positive_fixes.rs"]
mod false_positive_fixes;
#[path = "integration_test/framework_convention_coverage_astro_gatsby.rs"]
mod framework_convention_coverage_astro_gatsby;
#[path = "integration_test/framework_convention_coverage_common.rs"]
mod framework_convention_coverage_common;
#[path = "integration_test/framework_convention_coverage_docusaurus.rs"]
mod framework_convention_coverage_docusaurus;
#[path = "integration_test/framework_convention_coverage_electron.rs"]
mod framework_convention_coverage_electron;
#[path = "integration_test/framework_convention_coverage_expo_tanstack.rs"]
mod framework_convention_coverage_expo_tanstack;
#[path = "integration_test/framework_convention_coverage_mintlify.rs"]
mod framework_convention_coverage_mintlify;
#[path = "integration_test/framework_convention_coverage_router.rs"]
mod framework_convention_coverage_router;
#[path = "integration_test/framework_convention_coverage_vitepress.rs"]
mod framework_convention_coverage_vitepress;
#[path = "integration_test/frameworks.rs"]
mod frameworks;
#[path = "integration_test/graphql_imports.rs"]
mod graphql_imports;
#[path = "integration_test/hono_html_tagged_template.rs"]
mod hono_html_tagged_template;
#[path = "integration_test/html_entry.rs"]
mod html_entry;
#[path = "integration_test/issue_546_storybook_runtime_resources.rs"]
mod issue_546_storybook_runtime_resources;
#[path = "integration_test/jsx_assets_and_jsdoc.rs"]
mod jsx_assets_and_jsdoc;
#[path = "integration_test/member_detection.rs"]
mod member_detection;
#[path = "integration_test/nx_project_json.rs"]
mod nx_project_json;
#[path = "integration_test/redwoodsdk.rs"]
mod redwoodsdk;
#[path = "integration_test/rules_config.rs"]
mod rules_config;
#[path = "integration_test/sfc_parsing.rs"]
mod sfc_parsing;
#[path = "integration_test/unreachable_exports.rs"]
mod unreachable_exports;
#[path = "integration_test/workspaces.rs"]
mod workspaces;

#[path = "integration_test/boundary_violations.rs"]
mod boundary_violations;
#[path = "integration_test/config_file_loading.rs"]
mod config_file_loading;
#[path = "integration_test/css_modules_unused.rs"]
mod css_modules_unused;
#[path = "integration_test/private_type_leaks.rs"]
mod private_type_leaks;
#[path = "integration_test/production_mode.rs"]
mod production_mode;
#[path = "integration_test/re_export_chains.rs"]
mod re_export_chains;
#[path = "integration_test/stale_suppressions.rs"]
mod stale_suppressions;
#[path = "integration_test/suppression_comments.rs"]
mod suppression_comments;
#[path = "integration_test/test_only_deps.rs"]
mod test_only_deps;
#[path = "integration_test/type_only_deps.rs"]
mod type_only_deps;
#[path = "integration_test/unused_enum_members.rs"]
mod unused_enum_members;
#[path = "integration_test/web_components.rs"]
mod web_components;
#[path = "integration_test/workspace_cross_imports.rs"]
mod workspace_cross_imports;
#[path = "integration_test/workspace_internal_deps.rs"]
mod workspace_internal_deps;

#[path = "integration_test/inheritance_members.rs"]
mod inheritance_members;
#[path = "integration_test/issue_346_static_factory_method.rs"]
mod issue_346_static_factory_method;
#[path = "integration_test/issue_604_vite_rollup_path_helpers.rs"]
mod issue_604_vite_rollup_path_helpers;
#[path = "integration_test/issue_605_new_class_member.rs"]
mod issue_605_new_class_member;
#[path = "integration_test/issue_616_browser_extension_manifest.rs"]
mod issue_616_browser_extension_manifest;
#[path = "integration_test/issue_617_obsidian_plugin.rs"]
mod issue_617_obsidian_plugin;
#[path = "integration_test/issue_752_svelte_typed_props.rs"]
mod issue_752_svelte_typed_props;
#[path = "integration_test/issue_758_danger_no_dep.rs"]
mod issue_758_danger_no_dep;
#[path = "integration_test/lit_custom_element.rs"]
mod lit_custom_element;
#[path = "integration_test/scoped_used_class_members.rs"]
mod scoped_used_class_members;
#[path = "integration_test/scss_partials.rs"]
mod scss_partials;
#[path = "integration_test/super_method_calls.rs"]
mod super_method_calls;

#[path = "integration_test/angular_template_members.rs"]
mod angular_template_members;
#[path = "integration_test/arrow_wrapped_imports.rs"]
mod arrow_wrapped_imports;
#[path = "integration_test/bin_script_deps.rs"]
mod bin_script_deps;
#[path = "integration_test/entry_export_validation.rs"]
mod entry_export_validation;
#[path = "integration_test/issue_195_non_source_entry_points.rs"]
mod issue_195_non_source_entry_points;
#[path = "integration_test/issue_317_namespace_barrel_ignore_exports.rs"]
mod issue_317_namespace_barrel_ignore_exports;
#[path = "integration_test/issue_329_pnpm_catalog.rs"]
mod issue_329_pnpm_catalog;
#[path = "integration_test/issue_334_unresolved_catalog_ref.rs"]
mod issue_334_unresolved_catalog_ref;
#[path = "integration_test/issue_336_unused_overrides.rs"]
mod issue_336_unused_overrides;
#[path = "integration_test/issue_358_custom_eslint_config.rs"]
mod issue_358_custom_eslint_config;
#[path = "integration_test/issue_359_empty_catalog_group.rs"]
mod issue_359_empty_catalog_group;
#[path = "integration_test/issue_396_397_399_typeof_import_and_new_url.rs"]
mod issue_396_397_399_typeof_import_and_new_url;
#[path = "integration_test/issue_463_glob_validation.rs"]
mod issue_463_glob_validation;
#[path = "integration_test/issue_515_re_export_cycles.rs"]
mod issue_515_re_export_cycles;
#[path = "integration_test/issue_601_vitest_test_alias.rs"]
mod issue_601_vitest_test_alias;
#[path = "integration_test/issue_607_oxlint_js_plugins.rs"]
mod issue_607_oxlint_js_plugins;
#[path = "integration_test/issue_610_contentlayer_plugin.rs"]
mod issue_610_contentlayer_plugin;
#[path = "integration_test/issue_612_wxt_plugin.rs"]
mod issue_612_wxt_plugin;
#[path = "integration_test/issue_619_vite_react_babel_plugins.rs"]
mod issue_619_vite_react_babel_plugins;
#[path = "integration_test/issue_621_playwright_webserver.rs"]
mod issue_621_playwright_webserver;
#[path = "integration_test/issue_622_varlock_plugin.rs"]
mod issue_622_varlock_plugin;
#[path = "integration_test/issue_624_supabase_edge.rs"]
mod issue_624_supabase_edge;
#[path = "integration_test/issue_625_k6_plugin.rs"]
mod issue_625_k6_plugin;
#[path = "integration_test/issue_629_opencode_plugin.rs"]
mod issue_629_opencode_plugin;
#[path = "integration_test/issue_635_scaffold_template_assets.rs"]
mod issue_635_scaffold_template_assets;
#[path = "integration_test/issue_638_node_script_entrypoints.rs"]
mod issue_638_node_script_entrypoints;
#[path = "integration_test/lexical_nodes.rs"]
mod lexical_nodes;
#[path = "integration_test/script_multiplexers.rs"]
mod script_multiplexers;
#[path = "integration_test/visibility_tags.rs"]
mod visibility_tags;

#[path = "integration_test/ember_classic.rs"]
mod ember_classic;

#[path = "integration_test/issue_620_error_subclass_name.rs"]
mod issue_620_error_subclass_name;

#[path = "integration_test/issue_609_velite.rs"]
mod issue_609_velite;
#[path = "integration_test/issue_704_auto_import_components.rs"]
mod issue_704_auto_import_components;
#[path = "integration_test/issue_744_tsdown_config.rs"]
mod issue_744_tsdown_config;
#[path = "integration_test/pkg_utils_plugin.rs"]
mod pkg_utils_plugin;
