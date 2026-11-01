//! A plugin with no activation condition never activates, so config load warns.
//!
//! The warning goes to stderr only. It changes no finding, no stdout and no
//! exit code. The file also fixes the behavior of a `framework[]` entry with
//! the `support` role for tool scripts: it clears
//! `dev-dependencies-in-production` only when the glob is not in `entry`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::process::Command;

use crate::common::{CommandOutput, fallow_bin, parse_json, scrub_analysis_env};
use tempfile::TempDir;

const WARNING: &str = "plugin 'tools': no detection or enablers declared, so the plugin can \
                       never activate. Add one of them, then run `fallow plugin-check`.";
const REASON: &str = "no detection or enablers declared, so the plugin can never activate";

const NO_PLUGIN: &str =
    r#"{"entry":["src/index.js"],"rules":{"dev-dependencies-in-production":"error"}}"#;
const ENTRY_GLOB: &str = r#"{"entry":["src/index.js","tools/*.js"],
    "rules":{"dev-dependencies-in-production":"error"}}"#;
const INERT: &str = r#"{"entry":["src/index.js"],"rules":{"dev-dependencies-in-production":"error"},
    "framework":[{"name":"tools","entryPoints":["tools/**/*.js"]}]}"#;
const EMPTY_ENABLERS: &str = r#"{"entry":["src/index.js"],"framework":[
    {"name":"tools","enablers":[],"entryPoints":["tools/**/*.js"]}]}"#;
const UNMET_ENABLER: &str = r#"{"entry":["src/index.js"],"framework":[
    {"name":"tools","enablers":["not-installed"],"entryPoints":["tools/**/*.js"]}]}"#;
const UNMET_DETECTION: &str = r#"{"entry":["src/index.js"],"framework":[
    {"name":"tools","detection":{"type":"dependency","package":"not-installed"},
     "entryPoints":["tools/**/*.js"]}]}"#;
const SUPPORT_ENTRY: &str = r#"{"entry":["src/index.js"],
    "rules":{"dev-dependencies-in-production":"error"},
    "framework":[{"name":"tools","detection":{"type":"fileExists","pattern":"tools/**/*.js"},
     "entryPoints":["tools/**/*.js"],"entryPointRole":"support"}]}"#;
const SUPPORT_ENTRY_AND_ENTRY_GLOB: &str = r#"{"entry":["src/index.js","tools/*.js"],
    "rules":{"dev-dependencies-in-production":"error"},
    "framework":[{"name":"tools","detection":{"type":"fileExists","pattern":"tools/**/*.js"},
     "entryPoints":["tools/**/*.js"],"entryPointRole":"support"}]}"#;

const JSON_DEAD_CODE: &[&str] = &["dead-code", "--format", "json"];

/// A project where `tools/report.js` imports a devDependency and is the only
/// consumer of `src/toolhelper.js`.
fn project(config: &str) -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let files = [
        (
            "package.json",
            r#"{"name":"inert-fx","private":true,"type":"module","devDependencies":{"left-pad":"1.3.0"}}"#,
        ),
        ("src/index.js", "export const app = 1;\n"),
        ("src/toolhelper.js", "export const usedByToolOnly = 1;\n"),
        (
            "tools/report.js",
            "import leftPad from 'left-pad';\n\
             import { usedByToolOnly } from '../src/toolhelper.js';\n\
             console.log(leftPad('x', 4), usedByToolOnly);\n",
        ),
        (".fallowrc.json", config),
    ];
    for (path, content) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, content).expect("write");
    }
    dir
}

/// Run with the ambient `RUST_LOG` removed, so the CLI default filter decides
/// what reaches stderr.
fn run(dir: &TempDir, args: &[&str], env: &[(&str, &str)]) -> CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("NO_COLOR", "1")
        .env_remove("RUST_LOG")
        .env_remove("FALLOW_QUIET");
    scrub_analysis_env(&mut cmd);
    cmd.envs(env.iter().copied())
        .args(args)
        .arg("--root")
        .arg(dir.path())
        .arg("--no-cache");
    let output = cmd.output().expect("run fallow");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn warning_count(output: &CommandOutput) -> usize {
    output.stderr.matches(REASON).count()
}

fn assert_warns_once(output: &CommandOutput) {
    assert!(
        output.stderr.contains(WARNING),
        "expected warning:\n{WARNING}\nstderr:\n{}",
        output.stderr
    );
    assert_eq!(warning_count(output), 1, "stderr:\n{}", output.stderr);
}

fn names(output: &CommandOutput, section: &str, field: &str) -> Vec<String> {
    let json = parse_json(output);
    let items = json[section]
        .as_array()
        .unwrap_or_else(|| panic!("{section} is not an array: {json}"));
    let mut names: Vec<String> = items
        .iter()
        .filter_map(|item| item[field].as_str().map(str::to_owned))
        .collect();
    names.sort();
    names
}

#[test]
fn entry_without_condition_warns_once_on_stderr() {
    let output = run(&project(INERT), &["dead-code"], &[]);
    assert_warns_once(&output);
    assert!(
        !output.stdout.contains(REASON),
        "stdout:\n{}",
        output.stdout
    );
}

#[test]
fn empty_enablers_list_warns_like_absent_enablers() {
    assert_warns_once(&run(&project(EMPTY_ENABLERS), &["dead-code"], &[]));
}

#[test]
fn bare_command_warns_once() {
    assert_warns_once(&run(&project(INERT), &[], &[]));
}

#[test]
fn plugin_file_without_condition_warns() {
    let dir = project(NO_PLUGIN);
    let plugins = dir.path().join(".fallow/plugins");
    std::fs::create_dir_all(&plugins).expect("plugins dir");
    std::fs::write(
        plugins.join("tools.json"),
        r#"{"name":"tools","entryPoints":["tools/**/*.js"]}"#,
    )
    .expect("plugin file");
    assert_warns_once(&run(&dir, &["dead-code"], &[]));
}

#[test]
fn entry_with_a_condition_does_not_warn_even_when_inactive() {
    for config in [UNMET_ENABLER, UNMET_DETECTION, SUPPORT_ENTRY] {
        let output = run(&project(config), &["dead-code"], &[]);
        assert_eq!(warning_count(&output), 0, "{config}\n{}", output.stderr);
    }
}

#[test]
fn quiet_modes_keep_the_warning() {
    let dir = project(INERT);
    assert_warns_once(&run(&dir, &["dead-code", "--quiet"], &[]));
    assert_warns_once(&run(&dir, &["dead-code", "--ci"], &[]));
    assert_warns_once(&run(&dir, &["dead-code"], &[("FALLOW_QUIET", "1")]));
}

#[test]
fn warning_changes_no_finding_and_no_exit_code() {
    let inert = run(&project(INERT), JSON_DEAD_CODE, &[]);
    let plain = run(&project(NO_PLUGIN), JSON_DEAD_CODE, &[]);
    assert_warns_once(&inert);
    assert_eq!(warning_count(&plain), 0, "stderr:\n{}", plain.stderr);
    assert_eq!(inert.code, plain.code);
    assert_eq!(
        names(&inert, "unused_files", "path"),
        vec!["src/toolhelper.js".to_owned(), "tools/report.js".to_owned()],
    );
    assert_eq!(
        names(&inert, "unused_files", "path"),
        names(&plain, "unused_files", "path"),
    );
    assert_eq!(
        names(&inert, "unused_exports", "export_name"),
        names(&plain, "unused_exports", "export_name"),
    );
}

#[test]
fn plugin_check_reports_the_same_reason() {
    let output = run(&project(INERT), &["plugin-check", "--format", "json"], &[]);
    let json = parse_json(&output);
    let plugin = &json["plugins"][0];
    assert_eq!(plugin["active"], false);
    assert_eq!(plugin["activation_requirement"], REASON);
}

#[test]
fn support_entry_clears_finding_and_keeps_tool_imports_alive() {
    let output = run(&project(SUPPORT_ENTRY), JSON_DEAD_CODE, &[]);
    assert_eq!(output.code, 0, "stdout:\n{}", output.stdout);
    assert!(names(&output, "dev_dependencies_in_production", "package_name").is_empty());
    assert!(names(&output, "unused_files", "path").is_empty());
    assert!(names(&output, "unused_exports", "export_name").is_empty());
}

#[test]
fn glob_that_stays_in_entry_keeps_the_runtime_role() {
    let output = run(&project(SUPPORT_ENTRY_AND_ENTRY_GLOB), JSON_DEAD_CODE, &[]);
    assert_eq!(output.code, 1, "stdout:\n{}", output.stdout);
    assert_eq!(
        names(&output, "dev_dependencies_in_production", "package_name"),
        vec!["left-pad".to_owned()],
    );
}

#[test]
fn human_section_links_to_the_docs_for_the_finding() {
    let output = run(&project(ENTRY_GLOB), &["dead-code"], &[]);
    assert!(
        output.stdout.contains(
            "https://fallow.tools/docs/explanations/dead-code/#dev-dependencies-in-production"
        ),
        "stdout:\n{}",
        output.stdout
    );
}
