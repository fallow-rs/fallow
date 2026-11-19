//! The editor setting `fallow.production` gives the rules of production mode,
//! as `fallow dead-code --production` does. Without the setting, duplication
//! uses its own production mode from the config, as `fallow dupes` does.

use std::fmt::Write as _;

use super::*;

/// The session of `root` with the editor setting `production`.
fn load_session(root: &Path, production: Option<bool>) -> fallow_api::EditorAnalysisSession {
    let options = LspInitializationOptions {
        production,
        ..LspInitializationOptions::default()
    };
    let inputs = crate::run_inputs::run_inputs(&options, root, fallow_config::EnvSnapshot::none());
    analysis::load_project_session(root, &inputs)
        .expect("session loads")
        .session
}

fn unused_dev_dependencies(root: &Path, production_override: Option<bool>) -> Vec<String> {
    let session = load_session(root, production_override);
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
    // The resolver gives the canonical root, as the server does.
    let root = &dunce::canonicalize(dir.path()).expect("canonical root");
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

/// A function of 40 lines, above the default token and line minimums.
fn clone_source() -> String {
    let mut source =
        String::from("export function repeated(input: number): number {\n  let total = input;\n");
    for step in 0..36 {
        writeln!(source, "  total = total * {} + {};", step + 2, step * 3 + 1)
            .expect("write to string");
    }
    source.push_str("  return total;\n}\n");
    source
}

/// The files of each clone group that the editor session reports for a
/// project with `production` set to `production`. Production mode leaves the
/// `tests/` directory out, and the default duplication ignores do not.
fn clone_files(production: &str) -> Vec<Vec<String>> {
    let dir = tempfile::tempdir().expect("temp dir");
    // The resolver gives the canonical root, as the server does.
    let root = &dunce::canonicalize(dir.path()).expect("canonical root");
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(root.join("package.json"), "{}").expect("write package");
    std::fs::write(
        root.join(".fallowrc.json"),
        format!(r#"{{"production":{production}}}"#),
    )
    .expect("write config");
    std::fs::create_dir_all(root.join("tests")).expect("create tests dir");
    std::fs::write(root.join("tests/foo.ts"), clone_source()).expect("write test file");
    std::fs::write(root.join("src/foo.ts"), clone_source()).expect("write source file");

    let session = load_session(root, None);
    let duplicates = session.config().duplicates.clone();
    let output = session
        .analyze_project_with_changed_files(&duplicates, false, None)
        .expect("analysis runs");
    output
        .duplication
        .clone_groups
        .iter()
        .map(|group| {
            let mut files: Vec<String> = group
                .instances
                .iter()
                .map(|instance| {
                    let path = instance.file.strip_prefix(root).unwrap_or(&instance.file);
                    path.to_string_lossy().replace('\\', "/")
                })
                .collect();
            files.sort();
            files
        })
        .collect()
}

#[test]
fn editor_auto_mode_dupes_follow_their_own_mode() {
    let both = vec![vec!["src/foo.ts".to_owned(), "tests/foo.ts".to_owned()]];
    assert_eq!(
        clone_files(r#"{"deadCode":true,"dupes":false}"#),
        both,
        "duplication keeps the test file when only dead code is production"
    );
    assert!(
        clone_files(r#"{"deadCode":false,"dupes":true}"#).is_empty(),
        "production duplication leaves the test file out"
    );
    assert_eq!(
        clone_files("false"),
        both,
        "the boolean form keeps the test file"
    );
    assert!(
        clone_files("true").is_empty(),
        "the boolean form leaves the test file out"
    );
}
