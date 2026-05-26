//! Credit `@iconify-json/<prefix>` packages from static icon strings (issue #608).
//!
//! Iconify icon components consume an icon set through a build-time string name
//! (`<Icon name="jam:github" />`) rather than a JavaScript `import`, so the
//! `@iconify-json/<prefix>` package supplying that collection is invisible to
//! import-graph analysis and would be reported as an unused dependency.
//!
//! The extraction layer records the collection prefixes seen in markup on
//! [`ModuleInfo::iconify_prefixes`]. This bridge maps each prefix to its
//! `@iconify-json/<prefix>` package and returns the list of packages to credit
//! as referenced dependencies, GATED on the project actually declaring an
//! Iconify-ecosystem dependency. Crediting only exempts a declared dependency
//! from "unused"; it never produces a finding, so a stray non-icon
//! `name="foo:bar"` whose `@iconify-json/foo` is not declared is a no-op.

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

/// Map deduped Iconify collection prefixes to sorted `@iconify-json/<prefix>`
/// package names.
fn iconify_packages_for_prefixes<'a>(prefixes: impl Iterator<Item = &'a str>) -> Vec<String> {
    let unique: FxHashSet<&str> = prefixes.collect();
    let mut packages: Vec<String> = unique
        .into_iter()
        .map(|prefix| format!("@iconify-json/{prefix}"))
        .collect();
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
    iconify_packages_for_prefixes(
        modules
            .iter()
            .flat_map(|module| module.iconify_prefixes.iter().map(String::as_str)),
    )
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
        let packages =
            iconify_packages_for_prefixes(["jam", "ic", "jam", "simple-icons"].into_iter());
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
