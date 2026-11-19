//! The editor setting `fallow.production` gives the rules of production mode,
//! as `fallow dead-code --production` does.

use super::*;

use crate::session_store::SessionKey;

fn unused_dev_dependencies(root: &Path, production_override: Option<bool>) -> Vec<String> {
    let key = SessionKey {
        config_path: None,
        allow_remote_extends: false,
        production_override,
    };
    let (session, _) = analysis::load_project_session(root, &key).expect("session loads");
    let output = session
        .analyze_project_with(&fallow_config::DuplicatesConfig::default(), false)
        .expect("analysis runs");
    output
        .dead_code
        .results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.clone())
        .collect()
}

#[test]
fn production_setting_turns_unused_dev_dependencies_off() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"lsp-production-rules","private":true,"main":"src/index.ts","devDependencies":{"unused-dev-tool":"1.0.0"}}"#,
    )
    .expect("write package");
    std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").expect("write index");

    assert_eq!(
        unused_dev_dependencies(root, None),
        vec!["unused-dev-tool".to_owned()],
        "without production mode the dev dependency is unused"
    );
    assert!(
        unused_dev_dependencies(root, Some(true)).is_empty(),
        "production mode turns unused-dev-dependencies off"
    );
}
