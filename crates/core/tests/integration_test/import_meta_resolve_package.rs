//! `import.meta.resolve('pkg')` returns the URL of an installed package. Code
//! hands that URL to a consumer that fallow cannot see, such as a child
//! process or a file read. The call uses the package, so the dependency is
//! not unused. The specifier follows the same limits as `require.resolve`:
//! a bare package name, `<pkg>/package.json` and a deeper subpath such as
//! `pkg/lib/tsc` all credit the package.

use super::common::{create_config, fixture_path};

#[cfg_attr(miri, ignore)]
#[test]
fn resolve_calls_credit_the_resolved_package() {
    let root = fixture_path("import-meta-resolve-package");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    unused.sort_unstable();
    assert_eq!(
        unused,
        ["local-meta-pkg", "scoped-unused"],
        "import.meta.resolve and require.resolve credit the package of a bare name or of any subpath"
    );
}
