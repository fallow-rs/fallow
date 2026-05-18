//! Issue #358: ESLint config in a custom (hidden) location is detected as an
//! entry point but the walker skips the hidden directory, so its imports never
//! become graph edges and dependencies imported only from the custom config are
//! reported as `unused-dependency`.
//!
//! The fix collects hidden directory names referenced from `package.json#scripts`
//! into the existing `HiddenDirScope` mechanism so the walker traverses them.

use super::common::{create_config, fixture_path};

#[test]
fn imports_in_custom_location_eslint_config_credit_their_packages() {
    let root = fixture_path("issue-358-custom-eslint-config");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // Before the fix: `@eslint/js` shows up in unused_dependencies because
    // `.config/eslint.config.js` was never parsed (hidden directory skipped).
    let unused_deps: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_deps.contains(&"@eslint/js"),
        "@eslint/js is imported by .config/eslint.config.js; should not be unused. \
         Got unused_dependencies: {unused_deps:?}"
    );
    assert!(
        !unused_deps.contains(&"eslint"),
        "eslint provides the `eslint/config` subpath imported by the custom config; \
         should not be unused. Got: {unused_deps:?}"
    );
}

#[test]
fn custom_location_config_file_itself_is_not_unused() {
    let root = fixture_path("issue-358-custom-eslint-config");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|p| p.ends_with(".config/eslint.config.js")),
        ".config/eslint.config.js is an entry point referenced from package.json#scripts \
         and must not be reported as unused. Got: {unused_files:?}"
    );
}
