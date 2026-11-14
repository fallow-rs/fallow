//! A `url()` in a stylesheet or an SFC `<style>` block that names a file of a
//! declared package, such as an `@font-face` `src` of a font package, counts
//! as a use of that package.

use super::common::{create_config, fixture_path};

#[test]
fn css_url_package_subpath_credits_declared_dependency() {
    let root = fixture_path("css-url-package-credit");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert_eq!(unused, vec!["unused-control"]);

    // `@images/icons` is a bundler alias and `@app/img` a tsconfig path, so
    // the url() values that name them report nothing. The JS import of an
    // undeclared package is the control.
    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert_eq!(unlisted, vec!["undeclared-control"]);

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();
    assert_eq!(unresolved, vec!["./missing-control"]);

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert_eq!(unused_files, vec!["src/orphan-control.ts"]);
}
