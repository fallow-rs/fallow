//! Credit `@iconify-json/<prefix>` packages from static icon strings (issues
//! #608 and #955).
//!
//! Iconify icon components consume an icon set through a build-time string name
//! (`<Icon name="jam:github" />`) rather than a JavaScript `import`, so the
//! `@iconify-json/<prefix>` package supplying that collection is invisible to
//! import-graph analysis and would be reported as an unused dependency.
//!
//! The extraction layer records exact collection prefixes seen in markup on
//! [`ModuleInfo::iconify_prefixes`] and Nuxt UI icon class suffixes seen in Vue
//! script-side object data on [`ModuleInfo::iconify_icon_names`]. This bridge
//! maps those values to declared `@iconify-json/<prefix>` packages and returns
//! the list of packages to credit as referenced dependencies, GATED on the
//! project actually declaring an Iconify-ecosystem dependency. Crediting only
//! exempts a declared dependency from "unused"; it never produces a finding.

use fallow_config::{PackageJson, WorkspaceInfo};
use rustc_hash::FxHashSet;

use crate::extract::ModuleInfo;

/// Exact package names that mark a project as using the Iconify ecosystem.
const ICONIFY_ECOSYSTEM_EXACT: &[&str] = &["astro-icon", "unplugin-icons"];

/// Scoped-package prefixes that mark a project as using the Iconify ecosystem
/// (the icon-set packages themselves and the framework wrappers).
const ICONIFY_ECOSYSTEM_SCOPES: &[&str] = &["@iconify/", "@iconify-json/", "@iconify-icons/"];

/// Whether a declared dependency name belongs to the Iconify ecosystem.
fn is_iconify_ecosystem_dep(name: &str) -> bool {
    ICONIFY_ECOSYSTEM_EXACT.contains(&name)
        || ICONIFY_ECOSYSTEM_SCOPES
            .iter()
            .any(|scope| name.starts_with(scope))
}

/// Whether the root package or any workspace declares an Iconify-ecosystem dep.
fn iconify_ecosystem_present(pkg: Option<&PackageJson>, workspaces: &[WorkspaceInfo]) -> bool {
    let declares_iconify = |pkg: &PackageJson| {
        pkg.all_dependency_names()
            .iter()
            .any(|name| is_iconify_ecosystem_dep(name))
    };
    if pkg.is_some_and(declares_iconify) {
        return true;
    }
    workspaces.iter().any(|ws| {
        PackageJson::load(&ws.root.join("package.json"))
            .ok()
            .is_some_and(|pkg| declares_iconify(&pkg))
    })
}

fn collect_declared_iconify_json_packages(
    pkg: Option<&PackageJson>,
    workspaces: &[WorkspaceInfo],
) -> FxHashSet<String> {
    let mut packages = FxHashSet::default();
    let mut collect = |pkg: &PackageJson| {
        packages.extend(
            pkg.all_dependency_names()
                .into_iter()
                .filter(|name| name.starts_with("@iconify-json/")),
        );
    };
    if let Some(pkg) = pkg {
        collect(pkg);
    }
    for ws in workspaces {
        if let Ok(pkg) = PackageJson::load(&ws.root.join("package.json")) {
            collect(&pkg);
        }
    }
    packages
}

fn declared_iconify_collection_names(declared: &FxHashSet<String>) -> Vec<String> {
    let mut collections: Vec<String> = declared
        .iter()
        .filter_map(|package| package.strip_prefix("@iconify-json/"))
        .map(ToOwned::to_owned)
        .collect();
    collections.sort_unstable_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    collections
}

fn icon_name_matches_collection(icon_name: &str, collection: &str) -> bool {
    icon_name
        .strip_prefix(collection)
        .is_some_and(|rest| rest.starts_with('-') && rest.len() > 1)
}

/// Map extracted Iconify strings to sorted declared `@iconify-json/<prefix>`
/// package names.
fn iconify_packages_for_modules(
    modules: &[ModuleInfo],
    declared: &FxHashSet<String>,
) -> Vec<String> {
    let collections = declared_iconify_collection_names(declared);
    let mut packages = FxHashSet::default();
    for module in modules {
        for prefix in &module.iconify_prefixes {
            let package = format!("@iconify-json/{prefix}");
            if declared.contains(&package) {
                packages.insert(package);
            }
        }
        for icon_name in &module.iconify_icon_names {
            if let Some(collection) = collections
                .iter()
                .find(|collection| icon_name_matches_collection(icon_name, collection))
            {
                packages.insert(format!("@iconify-json/{collection}"));
            }
        }
    }
    let mut packages: Vec<String> = packages.into_iter().collect();
    packages.sort_unstable();
    packages
}

/// Collect `@iconify-json/<prefix>` packages to credit as referenced
/// dependencies, derived from static icon strings seen across `modules`.
///
/// Returns an empty `Vec` (cheap no-op) unless the project declares an
/// Iconify-ecosystem dependency, so non-Iconify projects pay nothing.
pub(super) fn collect_iconify_referenced_deps(
    modules: &[ModuleInfo],
    pkg: Option<&PackageJson>,
    workspaces: &[WorkspaceInfo],
) -> Vec<String> {
    if !iconify_ecosystem_present(pkg, workspaces) {
        return Vec::new();
    }
    let declared = collect_declared_iconify_json_packages(pkg, workspaces);
    iconify_packages_for_modules(modules, &declared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecosystem_dep_matches_exact_and_scoped_names() {
        assert!(is_iconify_ecosystem_dep("astro-icon"));
        assert!(is_iconify_ecosystem_dep("unplugin-icons"));
        assert!(is_iconify_ecosystem_dep("@iconify-json/jam"));
        assert!(is_iconify_ecosystem_dep("@iconify/react"));
        assert!(is_iconify_ecosystem_dep("@iconify-icons/mdi"));
    }

    #[test]
    fn ecosystem_dep_rejects_unrelated_names() {
        assert!(!is_iconify_ecosystem_dep("react"));
        assert!(!is_iconify_ecosystem_dep("astro"));
        assert!(!is_iconify_ecosystem_dep("astro-icons")); // not the real package name
        assert!(!is_iconify_ecosystem_dep("@iconifyish/jam"));
    }

    #[test]
    fn maps_prefixes_to_sorted_deduped_packages() {
        let module = ModuleInfo {
            iconify_prefixes: vec!["jam".to_string(), "ic".to_string(), "jam".to_string()],
            iconify_icon_names: vec!["simple-icons-github".to_string()],
            ..empty_module()
        };
        let declared = [
            "@iconify-json/jam".to_string(),
            "@iconify-json/ic".to_string(),
            "@iconify-json/simple-icons".to_string(),
        ]
        .into_iter()
        .collect();
        let packages = iconify_packages_for_modules(&[module], &declared);
        assert_eq!(
            packages,
            vec![
                "@iconify-json/ic",
                "@iconify-json/jam",
                "@iconify-json/simple-icons",
            ]
        );
    }

    #[test]
    fn nuxt_icon_names_use_longest_declared_collection_match() {
        let module = ModuleInfo {
            iconify_icon_names: vec!["simple-icons-github".to_string()],
            ..empty_module()
        };
        let declared = [
            "@iconify-json/simple".to_string(),
            "@iconify-json/simple-icons".to_string(),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            iconify_packages_for_modules(&[module], &declared),
            vec!["@iconify-json/simple-icons"]
        );
    }

    #[test]
    fn ecosystem_present_reads_root_package() {
        let iconify_pkg = PackageJson {
            dependencies: Some(
                [
                    ("@iconify-json/jam".to_string(), "^1".to_string()),
                    ("react".to_string(), "^18".to_string()),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        };
        assert!(iconify_ecosystem_present(Some(&iconify_pkg), &[]));

        let bare_pkg = PackageJson {
            dependencies: Some(
                [
                    ("react".to_string(), "^18".to_string()),
                    ("astro".to_string(), "^4".to_string()),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        };
        assert!(!iconify_ecosystem_present(Some(&bare_pkg), &[]));
    }

    fn empty_module() -> ModuleInfo {
        ModuleInfo {
            file_id: fallow_types::discover::FileId(1),
            exports: Vec::new(),
            imports: Vec::new(),
            re_exports: Vec::new(),
            dynamic_imports: Vec::new(),
            dynamic_import_patterns: Vec::new(),
            require_calls: Vec::new(),
            package_path_references: Vec::new(),
            member_accesses: Vec::new(),
            whole_object_uses: Vec::new(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            suppressions: Vec::new(),
            unknown_suppression_kinds: Vec::new(),
            unused_import_bindings: Vec::new(),
            type_referenced_import_bindings: Vec::new(),
            value_referenced_import_bindings: Vec::new(),
            line_offsets: Vec::new(),
            complexity: Vec::new(),
            flag_uses: Vec::new(),
            class_heritage: Vec::new(),
            injection_tokens: Vec::new(),
            local_type_declarations: Vec::new(),
            public_signature_type_references: Vec::new(),
            namespace_object_aliases: Vec::new(),
            iconify_prefixes: Vec::new(),
            iconify_icon_names: Vec::new(),
            auto_import_candidates: Vec::new(),
            directives: Vec::new(),
            client_only_dynamic_import_spans: Vec::new(),
            security_sinks: Vec::new(),
            security_sinks_skipped: 0,
            security_unresolved_callee_sites: Vec::new(),
            tainted_bindings: Vec::new(),
            sanitized_sink_args: Vec::new(),
            security_control_sites: Vec::new(),
            callee_uses: Vec::new(),
            misplaced_directives: Vec::new(),
            di_key_sites: Vec::new(),
            has_dynamic_provide: false,
            referenced_import_bindings: Vec::new(),
            component_props: Vec::new(),
            has_props_attrs_fallthrough: false,
            has_define_expose: false,
            has_define_model: false,
            has_unharvestable_props: false,
            component_emits: Vec::new(),
            angular_inputs: Vec::new(),
            angular_outputs: Vec::new(),
            has_unharvestable_emits: false,
            has_dynamic_emit: false,
            has_emit_whole_object_use: false,
            load_return_keys: Vec::new(),
            has_unharvestable_load: false,
            has_load_data_whole_use: false,
            has_page_data_store_whole_use: false,
            component_functions: Vec::new(),
            react_props: Vec::new(),
            hook_uses: Vec::new(),
            render_edges: Vec::new(),
        }
    }
}
