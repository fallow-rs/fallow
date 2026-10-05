//! A direct `require.resolve('pkg')` or `require.resolve('pkg/package.json')`
//! call with one string argument names a package at a known source location.
//! When the package is not in package.json, the call is an unlisted
//! dependency, the same as an import of the package. A package name that comes
//! from a resolver function, a loop over a static table, a call with a
//! `paths` option, or a specifier with a deeper subpath only credits the
//! dependency and is not reported.

use super::common::{create_config, fixture_path};

fn file_name(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

#[cfg_attr(miri, ignore)]
#[test]
fn literal_resolve_call_reports_an_unlisted_package() {
    let config = create_config(fixture_path("resolve-call-unlisted"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unlisted: Vec<(String, Vec<(String, u32)>)> = results
        .unlisted_dependencies
        .iter()
        .map(|finding| {
            let mut sites: Vec<(String, u32)> = finding
                .dep
                .imported_from
                .iter()
                .map(|site| (file_name(&site.path), site.line))
                .collect();
            sites.sort();
            (finding.dep.package_name.clone(), sites)
        })
        .collect();
    unlisted.sort();

    assert_eq!(
        unlisted,
        vec![(
            "unlisted-req".to_string(),
            vec![("both.js".to_string(), 1), ("index.js".to_string(), 4)],
        )],
        "only the package from a literal resolve call is unlisted, and the resolve-only file shows its call line"
    );
}
