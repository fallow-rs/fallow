//! Dynamic import shapes that name their targets without a plain string
//! literal, and `this`-returning method chains inside a class body.

use super::common::{create_config, fixture_path};

fn relative_paths(root: &std::path::Path, paths: Vec<&std::path::Path>) -> Vec<String> {
    let mut relative: Vec<String> = paths
        .into_iter()
        .map(|path| {
            path.strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    relative.sort();
    relative
}

/// A template literal with a literal suffix and no extension, such as
/// `` import(`./icons/__${name}__`) ``, loads `__first__.ts`: a bundler adds
/// the module extension to the request.
#[test]
fn template_suffix_without_extension_matches_module_files() {
    let config = create_config(fixture_path("dynamic-import-suffix-no-extension"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = relative_paths(
        &config.root,
        results
            .unused_files
            .iter()
            .map(|finding| finding.file.path.as_path())
            .collect(),
    );

    assert_eq!(
        unused,
        vec![
            "src/icons/__draft__.spec.ts".to_string(),
            "src/icons/other.ts".to_string(),
        ],
        "only files that no pattern loads stay unused"
    );
}

/// `const name = 'pkg'; import(name)` loads `pkg`. A parameter or a `let`
/// with the same role names no fixed module.
#[test]
fn const_string_specifier_credits_its_package() {
    let config = create_config(fixture_path("dynamic-import-const-specifier"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut unused_deps: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    unused_deps.sort_unstable();

    assert_eq!(
        unused_deps,
        vec![
            "caught-pkg",
            "hoisted-pkg",
            "let-pkg",
            "loop-pkg",
            "nested-caught-pkg",
            "shadowed-pkg",
            "sibling-pkg",
        ],
        "a const string specifier credits its package; a parameter or a let does not"
    );
    assert!(
        results.unused_files.is_empty(),
        "a const relative specifier loads its file: {:?}",
        results.unused_files
    );
}

/// `this.setId(id).reset().setLabel(label)` calls `reset` and `setLabel` on
/// the class itself when each method in the chain returns `this`.
#[test]
fn this_returning_method_chain_credits_each_member() {
    let config = create_config(fixture_path("this-returning-method-chain"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert_eq!(
        unused_members,
        vec!["Link.unusedChainable".to_string()],
        "every member that the chain calls is used"
    );
}
