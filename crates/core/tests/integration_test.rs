#[path = "integration_test/common.rs"]
mod common;

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
#[path = "integration_test/framework_convention_coverage_expo_tanstack.rs"]
mod framework_convention_coverage_expo_tanstack;
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
#[path = "integration_test/jsx_assets_and_jsdoc.rs"]
mod jsx_assets_and_jsdoc;
#[path = "integration_test/member_detection.rs"]
mod member_detection;
#[path = "integration_test/nx_project_json.rs"]
mod nx_project_json;
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
#[path = "integration_test/script_multiplexers.rs"]
mod script_multiplexers;
#[path = "integration_test/visibility_tags.rs"]
mod visibility_tags;
