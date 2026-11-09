//! Credit `@iconify-json/<prefix>` packages from static icon strings (issues
//! #608 and #955).
//!
//! Iconify icon components consume an icon set through a build-time string name
//! (`<Icon name="jam:github" />`) rather than a JavaScript `import`, so the
//! `@iconify-json/<prefix>` package supplying that collection is invisible to
//! import-graph analysis and would be reported as an unused dependency.
//!
//! unplugin-icons serves an icon as a virtual module (`~icons/<set>/<name>`).
//! An import of that form uses `@iconify-json/<set>`, the full `@iconify/json`
//! collection that unplugin-icons falls back to, and `unplugin-icons` itself.
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

use super::SetupAndDetectInput;
use crate::discover::FileId;
use crate::extract::ModuleInfo;
use crate::plugins::unplugin_icons::ICON_IMPORT_PREFIXES;
use crate::resolve::ResolvedModule;

/// The packages that an unplugin-icons import uses, apart from the icon set.
const ICON_IMPORT_PACKAGES: &[&str] = &["@iconify/json", "unplugin-icons"];

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
        PackageJson::load(&ws.root.join("package.json")).is_ok_and(|pkg| declares_iconify(&pkg))
    })
}

fn collect_declared_icon_packages(
    pkg: Option<&PackageJson>,
    workspaces: &[WorkspaceInfo],
) -> FxHashSet<String> {
    let mut packages = FxHashSet::default();
    let mut collect = |pkg: &PackageJson| {
        packages.extend(pkg.all_dependency_names().into_iter().filter(|name| {
            name.starts_with("@iconify-json/") || ICON_IMPORT_PACKAGES.contains(&name.as_str())
        }));
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

/// The icon set of an unplugin-icons import: `lucide` for `~icons/lucide/x`.
fn icon_import_set(specifier: &str) -> Option<&str> {
    let rest = ICON_IMPORT_PREFIXES
        .iter()
        .find_map(|prefix| specifier.strip_prefix(prefix))?;
    let (set, name) = rest.split_once('/')?;
    (!set.is_empty() && !name.is_empty()).then_some(set)
}

/// The import, re-export and dynamic import specifiers of a module. The
/// resolved module keeps the dynamic imports, which `ModuleInfo` releases
/// after resolution.
fn module_specifiers(module: &ResolvedModule) -> impl Iterator<Item = &str> {
    module
        .resolved_imports
        .iter()
        .chain(&module.resolved_dynamic_imports)
        .map(|import| import.info.source.as_str())
        .chain(
            module
                .re_exports
                .iter()
                .map(|re_export| re_export.info.source.as_str()),
        )
}

/// The packages that the unplugin-icons imports of `modules` use.
fn icon_import_packages<'a>(
    modules: impl IntoIterator<Item = &'a ResolvedModule>,
) -> FxHashSet<String> {
    let mut packages = FxHashSet::default();
    for set in modules
        .into_iter()
        .flat_map(module_specifiers)
        .filter_map(icon_import_set)
    {
        packages.insert(format!("@iconify-json/{set}"));
        packages.extend(
            ICON_IMPORT_PACKAGES
                .iter()
                .map(|package| (*package).to_string()),
        );
    }
    packages
}

/// Map extracted Iconify strings to declared `@iconify-json/<prefix>`
/// package names, add the declared packages of `icon_imports`, and sort.
fn iconify_packages_for_modules<'a>(
    modules: impl IntoIterator<Item = &'a ModuleInfo>,
    icon_imports: FxHashSet<String>,
    declared: &FxHashSet<String>,
) -> Vec<String> {
    let collections = declared_iconify_collection_names(declared);
    let mut packages = icon_imports;
    for module in modules {
        for prefix in &module.iconify_prefixes {
            packages.insert(format!("@iconify-json/{prefix}"));
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
    let mut packages: Vec<String> = packages
        .into_iter()
        .filter(|package| declared.contains(package))
        .collect();
    packages.sort_unstable();
    packages
}

/// Collect Iconify packages to credit as referenced dependencies, derived
/// from static icon strings and unplugin-icons imports. A production-mode
/// file that takes no part credits nothing.
///
/// Returns an empty `Vec` (cheap no-op) unless the project declares an
/// Iconify-ecosystem dependency, so non-Iconify projects pay nothing.
pub(super) fn collect_iconify_referenced_deps(input: &SetupAndDetectInput<'_, '_>) -> Vec<String> {
    if !iconify_ecosystem_present(input.pkg, input.workspaces) {
        return Vec::new();
    }
    let declared = collect_declared_icon_packages(input.pkg, input.workspaces);
    let takes_part = |file_id: &FileId| !input.excluded.contains(file_id);
    let modules = input.modules.iter().filter(|m| takes_part(&m.file_id));
    let resolved_modules = input
        .resolved_modules
        .iter()
        .filter(|m| takes_part(&m.file_id));
    iconify_packages_for_modules(modules, icon_import_packages(resolved_modules), &declared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::test_support::empty_module;

    #[test]
    fn ecosystem_dep_matches_exact_and_scoped_names() {
        assert!(is_iconify_ecosystem_dep("astro-icon"));
        assert!(is_iconify_ecosystem_dep("unplugin-icons"));
        assert!(is_iconify_ecosystem_dep("@iconify-json/jam"));
        assert!(is_iconify_ecosystem_dep("@iconify/react"));
        assert!(is_iconify_ecosystem_dep("@iconify-icons/mdi"));
    }

    #[test]
    fn icon_import_set_reads_the_set_of_each_import_form() {
        assert_eq!(icon_import_set("~icons/lucide/x"), Some("lucide"));
        assert_eq!(icon_import_set("virtual:icons/mdi/arrow-left"), Some("mdi"));
        assert_eq!(icon_import_set("virtual/icons/carbon/add"), Some("carbon"));
        assert_eq!(icon_import_set("~icons/lucide"), None);
        assert_eq!(icon_import_set("~icons//x"), None);
        assert_eq!(icon_import_set("lucide/x"), None);
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
        let packages = iconify_packages_for_modules(&[module], FxHashSet::default(), &declared);
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
            iconify_packages_for_modules(&[module], FxHashSet::default(), &declared),
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
}
