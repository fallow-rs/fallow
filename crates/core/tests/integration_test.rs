#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]
#![expect(
    deprecated,
    reason = "Core-internal policy: integration tests exercise the workspace path-dep fallow_core::analyze* surface; the deprecation warning targets external crates.io consumers"
)]

#[path = "integration_test/common.rs"]
mod common;

#[path = "integration_test/angular_ng_package.rs"]
mod angular_ng_package;
#[path = "integration_test/angular_unrendered_component.rs"]
mod angular_unrendered_component;
#[path = "integration_test/astro_unrendered_component.rs"]
mod astro_unrendered_component;
#[path = "integration_test/astro_unused_component_prop.rs"]
mod astro_unused_component_prop;
#[path = "integration_test/barrel_exports.rs"]
mod barrel_exports;
#[path = "integration_test/basic_analysis.rs"]
mod basic_analysis;
#[path = "integration_test/caching.rs"]
mod caching;
#[path = "integration_test/css_in_js_styled.rs"]
mod css_in_js_styled;
#[path = "integration_test/css_modules.rs"]
mod css_modules;
#[path = "integration_test/dependencies.rs"]
mod dependencies;
#[path = "integration_test/duplicate_prop_shape.rs"]
mod duplicate_prop_shape;
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
#[path = "integration_test/graph_cache_transparency.rs"]
mod graph_cache_transparency;
#[path = "integration_test/graphql_imports.rs"]
mod graphql_imports;
#[path = "integration_test/hono_html_tagged_template.rs"]
mod hono_html_tagged_template;
#[path = "integration_test/html_entry.rs"]
mod html_entry;
#[path = "integration_test/issue_1032_tsconfig_sibling_src_paths.rs"]
mod issue_1032_tsconfig_sibling_src_paths;
#[path = "integration_test/issue_1304_effect_schema_same_name.rs"]
mod issue_1304_effect_schema_same_name;
#[path = "integration_test/issue_546_storybook_runtime_resources.rs"]
mod issue_546_storybook_runtime_resources;
#[path = "integration_test/issue_914_pnpm_bare_binary.rs"]
mod issue_914_pnpm_bare_binary;
#[path = "integration_test/issue_948_vscode_provider_members.rs"]
mod issue_948_vscode_provider_members;
#[path = "integration_test/issue_952_package_path_resolution.rs"]
mod issue_952_package_path_resolution;
#[path = "integration_test/issue_954_pino_transport_target.rs"]
mod issue_954_pino_transport_target;
#[path = "integration_test/jsx_assets_and_jsdoc.rs"]
mod jsx_assets_and_jsdoc;
#[path = "integration_test/lit_unrendered_element.rs"]
mod lit_unrendered_element;
#[path = "integration_test/lit_unused_state.rs"]
mod lit_unused_state;
#[path = "integration_test/member_detection.rs"]
mod member_detection;
#[path = "integration_test/nx_project_json.rs"]
mod nx_project_json;
#[path = "integration_test/redwoodsdk.rs"]
mod redwoodsdk;
#[path = "integration_test/rspress_theme.rs"]
mod rspress_theme;
#[path = "integration_test/rules_config.rs"]
mod rules_config;
#[path = "integration_test/safe_analysis.rs"]
mod safe_analysis;
#[path = "integration_test/sfc_parsing.rs"]
mod sfc_parsing;
#[path = "integration_test/svelte_dead_event.rs"]
mod svelte_dead_event;
#[path = "integration_test/symlink_root_containment.rs"]
mod symlink_root_containment;
#[path = "integration_test/unreachable_exports.rs"]
mod unreachable_exports;
#[path = "integration_test/workspaces.rs"]
mod workspaces;

#[path = "integration_test/boundary_violations.rs"]
mod boundary_violations;
#[path = "integration_test/capability_e_route_exports.rs"]
mod capability_e_route_exports;
#[path = "integration_test/config_file_loading.rs"]
mod config_file_loading;
#[path = "integration_test/css_modules_unused.rs"]
mod css_modules_unused;
#[path = "integration_test/dev_dep_in_prod.rs"]
mod dev_dep_in_prod;
#[path = "integration_test/invalid_client_exports.rs"]
mod invalid_client_exports;
#[path = "integration_test/misplaced_directive.rs"]
mod misplaced_directive;
#[path = "integration_test/mixed_client_server_barrel.rs"]
mod mixed_client_server_barrel;
#[path = "integration_test/nextjs_route_tree.rs"]
mod nextjs_route_tree;
#[path = "integration_test/policy_violations.rs"]
mod policy_violations;
#[path = "integration_test/private_type_leaks.rs"]
mod private_type_leaks;
#[path = "integration_test/production_mode.rs"]
mod production_mode;
#[path = "integration_test/prop_drilling.rs"]
mod prop_drilling;
#[path = "integration_test/re_export_chains.rs"]
mod re_export_chains;
#[path = "integration_test/react_component_intel.rs"]
mod react_component_intel;
#[path = "integration_test/render_fan_in.rs"]
mod render_fan_in;
#[path = "integration_test/security_catalogue_categories.rs"]
mod security_catalogue_categories;
#[path = "integration_test/security_client_server_leak.rs"]
mod security_client_server_leak;
#[path = "integration_test/security_dangerous_html.rs"]
mod security_dangerous_html;
#[path = "integration_test/security_dead_code_cross_link.rs"]
mod security_dead_code_cross_link;
#[path = "integration_test/security_declarative_validation.rs"]
mod security_declarative_validation;
#[path = "integration_test/security_framework_entry_sources.rs"]
mod security_framework_entry_sources;
#[path = "integration_test/security_framework_sinks.rs"]
mod security_framework_sinks;
#[path = "integration_test/security_hardcoded_secret.rs"]
mod security_hardcoded_secret;
#[path = "integration_test/security_multihop_taint.rs"]
mod security_multihop_taint;
#[path = "integration_test/security_request_receivers.rs"]
mod security_request_receivers;
#[path = "integration_test/security_secret_to_network.rs"]
mod security_secret_to_network;
#[path = "integration_test/security_taint_confidence.rs"]
mod security_taint_confidence;
#[path = "integration_test/security_template_xss_sinks.rs"]
mod security_template_xss_sinks;
#[path = "integration_test/sfc_template_assets.rs"]
mod sfc_template_assets;
#[path = "integration_test/stale_suppressions.rs"]
mod stale_suppressions;
#[path = "integration_test/suppression_comments.rs"]
mod suppression_comments;
#[path = "integration_test/test_only_deps.rs"]
mod test_only_deps;
#[path = "integration_test/thin_wrapper.rs"]
mod thin_wrapper;
#[path = "integration_test/type_only_deps.rs"]
mod type_only_deps;
#[path = "integration_test/unprovided_injects.rs"]
mod unprovided_injects;
#[path = "integration_test/unrendered_components.rs"]
mod unrendered_components;
#[path = "integration_test/unrendered_components_namespace.rs"]
mod unrendered_components_namespace;
#[path = "integration_test/unused_component_emits.rs"]
mod unused_component_emits;
#[path = "integration_test/unused_component_io_angular.rs"]
mod unused_component_io_angular;
#[path = "integration_test/unused_component_props.rs"]
mod unused_component_props;
#[path = "integration_test/unused_enum_members.rs"]
mod unused_enum_members;
#[path = "integration_test/unused_load_data_keys.rs"]
mod unused_load_data_keys;
#[path = "integration_test/unused_react_props.rs"]
mod unused_react_props;
#[path = "integration_test/unused_server_actions.rs"]
mod unused_server_actions;
#[path = "integration_test/unused_store_members.rs"]
mod unused_store_members;
#[path = "integration_test/web_components.rs"]
mod web_components;
#[path = "integration_test/workspace_cross_imports.rs"]
mod workspace_cross_imports;
#[path = "integration_test/workspace_internal_deps.rs"]
mod workspace_internal_deps;

#[path = "integration_test/inheritance_members.rs"]
mod inheritance_members;
#[path = "integration_test/issue_1441_factory_return_member.rs"]
mod issue_1441_factory_return_member;
#[path = "integration_test/issue_1640_commit_and_tag_version_updater.rs"]
mod issue_1640_commit_and_tag_version_updater;
#[path = "integration_test/issue_1785_typed_property_hop.rs"]
mod issue_1785_typed_property_hop;
#[path = "integration_test/issue_1788_local_class_options.rs"]
mod issue_1788_local_class_options;
#[path = "integration_test/issue_1821_per_class_this_scoping.rs"]
mod issue_1821_per_class_this_scoping;
#[path = "integration_test/issue_1821_private_field_di.rs"]
mod issue_1821_private_field_di;
#[path = "integration_test/issue_1858_object_literal_factory_return.rs"]
mod issue_1858_object_literal_factory_return;
#[path = "integration_test/issue_1863_interface_property_dispatch.rs"]
mod issue_1863_interface_property_dispatch;
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
#[path = "integration_test/issue_753_oxlint_cli_tooling.rs"]
mod issue_753_oxlint_cli_tooling;
#[path = "integration_test/issue_758_danger_no_dep.rs"]
mod issue_758_danger_no_dep;
#[path = "integration_test/issue_772_workspace_plugin_merge.rs"]
mod issue_772_workspace_plugin_merge;
#[path = "integration_test/issue_845_instanceof_narrowing.rs"]
mod issue_845_instanceof_narrowing;
#[path = "integration_test/issue_910_structural_class_member_usage.rs"]
mod issue_910_structural_class_member_usage;

#[path = "integration_test/issue_844_usememo_instance.rs"]
mod issue_844_usememo_instance;
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
#[path = "integration_test/issue_1301_bun_catalog.rs"]
mod issue_1301_bun_catalog;
#[path = "integration_test/issue_1648_unused_prop_ignore_pattern.rs"]
mod issue_1648_unused_prop_ignore_pattern;
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
#[path = "integration_test/issue_462_tooling_catalogue.rs"]
mod issue_462_tooling_catalogue;
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
#[path = "integration_test/issue_754_eslint_meta_preset.rs"]
mod issue_754_eslint_meta_preset;
#[path = "integration_test/issue_811_vite_alias_identifier_spread.rs"]
mod issue_811_vite_alias_identifier_spread;
#[path = "integration_test/issue_818_prettier_pkg_json_string.rs"]
mod issue_818_prettier_pkg_json_string;
#[path = "integration_test/issue_820_vercel_ts_config.rs"]
mod issue_820_vercel_ts_config;
#[path = "integration_test/issue_823_pnpm_package_sources.rs"]
mod issue_823_pnpm_package_sources;
#[path = "integration_test/issue_868_ionic_lifecycle.rs"]
mod issue_868_ionic_lifecycle;
#[path = "integration_test/issue_873_firebase_messaging_sw.rs"]
mod issue_873_firebase_messaging_sw;
#[path = "integration_test/issue_956_playwright_pnpm_exec.rs"]
mod issue_956_playwright_pnpm_exec;
#[path = "integration_test/lexical_nodes.rs"]
mod lexical_nodes;
#[path = "integration_test/script_multiplexers.rs"]
mod script_multiplexers;
#[path = "integration_test/visibility_tags.rs"]
mod visibility_tags;
#[path = "integration_test/vue_options_api_prop_emit.rs"]
mod vue_options_api_prop_emit;

#[path = "integration_test/ember_classic.rs"]
mod ember_classic;

#[path = "integration_test/issue_620_error_subclass_name.rs"]
mod issue_620_error_subclass_name;

#[path = "integration_test/issue_1638_openlayers_dispatch.rs"]
mod issue_1638_openlayers_dispatch;

#[path = "integration_test/issue_1638_tostring_coercion.rs"]
mod issue_1638_tostring_coercion;

#[path = "integration_test/issue_1707_vue_vfor_class_member.rs"]
mod issue_1707_vue_vfor_class_member;

#[path = "integration_test/local_subclass_static_class_members.rs"]
mod local_subclass_static_class_members;

#[path = "integration_test/cross_file_factory_class_members.rs"]
mod cross_file_factory_class_members;

#[path = "integration_test/issue_1711_vue_props_vfor.rs"]
mod issue_1711_vue_props_vfor;

#[path = "integration_test/issue_1716_vue_ref_store_vfor.rs"]
mod issue_1716_vue_ref_store_vfor;

#[path = "integration_test/issue_1717_angular_template_url_class_member.rs"]
mod issue_1717_angular_template_url_class_member;

#[path = "integration_test/issue_1718_function_local_iteration.rs"]
mod issue_1718_function_local_iteration;

#[path = "integration_test/iteration_binding_element_types.rs"]
mod iteration_binding_element_types;

#[path = "integration_test/issue_1712_angular_for_class_member.rs"]
mod issue_1712_angular_for_class_member;

#[path = "integration_test/astro_template_map_class_member.rs"]
mod astro_template_map_class_member;

#[path = "integration_test/issue_843_nestjs_lifecycle.rs"]
mod issue_843_nestjs_lifecycle;

#[path = "integration_test/issue_609_velite.rs"]
mod issue_609_velite;
#[path = "integration_test/issue_704_auto_import_components.rs"]
mod issue_704_auto_import_components;
#[path = "integration_test/issue_739_script_auto_imports.rs"]
mod issue_739_script_auto_imports;
#[path = "integration_test/issue_740_pinia_store_auto_imports.rs"]
mod issue_740_pinia_store_auto_imports;
#[path = "integration_test/issue_744_tsdown_config.rs"]
mod issue_744_tsdown_config;
#[path = "integration_test/pkg_utils_plugin.rs"]
mod pkg_utils_plugin;
