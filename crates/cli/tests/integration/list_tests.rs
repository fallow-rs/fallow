#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{CommandOutput, fallow_bin, parse_json, run_fallow};

use std::fs;
use std::process::Command;

/// Run `fallow list` with the given args and return structured output.
fn run_list(fixture: &str, args: &[&str]) -> CommandOutput {
    run_fallow("list", fixture, args)
}

fn write_project_with_invalid_tanstack_route_ignore_pattern(root: &std::path::Path) {
    fs::create_dir_all(root.join("src/routes")).expect("create routes dir");
    fs::write(
        root.join("src/routes/index.tsx"),
        "export const Route = {}\n",
    )
    .expect("write route file");
    fs::write(
        root.join("package.json"),
        r#"{
            "name": "invalid-tanstack-regex",
            "dependencies": {
                "@tanstack/react-router": "latest",
                "@tanstack/router-plugin": "latest",
                "vite": "latest"
            }
        }"#,
    )
    .expect("write package json");
    fs::write(
        root.join("vite.config.ts"),
        r#"import { tanstackRouter } from "@tanstack/router-plugin/vite";

export default {
    plugins: [
        tanstackRouter({
            routeFileIgnorePattern: "^(?!layout\\.tsx$|__root\\.tsx$).+\\.tsx$",
        }),
    ],
};
"#,
    )
    .expect("write vite config");
}

fn run_fallow_combined_in_root(root: &std::path::Path, args: &[&str]) -> CommandOutput {
    let output = Command::new(fallow_bin())
        .arg("--root")
        .arg(root)
        .args(args)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run fallow binary");

    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

#[test]
fn list_show_all_json_includes_plugins_files_and_entry_points() {
    let output = run_list("basic-project", &["--format", "json"]);
    assert_eq!(
        output.code, 0,
        "expected exit code 0, stderr might have details"
    );

    let json = parse_json(&output);

    assert!(json.get("plugins").is_some(), "missing 'plugins' key");
    assert!(json.get("files").is_some(), "missing 'files' key");
    assert!(json.get("file_count").is_some(), "missing 'file_count' key");
    assert!(
        json.get("entry_points").is_some(),
        "missing 'entry_points' key"
    );
    assert!(
        json.get("entry_point_count").is_some(),
        "missing 'entry_point_count' key"
    );
    assert!(
        json.get("boundaries").is_none(),
        "show_all mode should omit 'boundaries' unless --boundaries is requested"
    );
}

#[test]
fn list_show_all_file_count_matches_files_array_length() {
    let output = run_list("basic-project", &["--format", "json"]);
    let json = parse_json(&output);

    let file_count = json["file_count"].as_u64().unwrap();
    let files_len = json["files"].as_array().unwrap().len() as u64;
    assert_eq!(
        file_count, files_len,
        "file_count ({file_count}) should match files array length ({files_len})"
    );
}

#[test]
fn list_show_all_entry_point_count_matches_array_length() {
    let output = run_list("basic-project", &["--format", "json"]);
    let json = parse_json(&output);

    let ep_count = json["entry_point_count"].as_u64().unwrap();
    let ep_len = json["entry_points"].as_array().unwrap().len() as u64;
    assert_eq!(
        ep_count, ep_len,
        "entry_point_count ({ep_count}) should match entry_points array length ({ep_len})"
    );
}

#[test]
fn list_plugins_only_json_omits_files_and_entry_points() {
    let output = run_list("basic-project", &["--plugins", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    assert!(json.get("plugins").is_some(), "should include 'plugins'");
    assert!(json.get("files").is_none(), "should omit 'files'");
    assert!(json.get("file_count").is_none(), "should omit 'file_count'");
    assert!(
        json.get("entry_points").is_none(),
        "should omit 'entry_points'"
    );
}

#[test]
fn list_files_only_json_omits_plugins_and_entry_points() {
    let output = run_list("basic-project", &["--files", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    assert!(json.get("plugins").is_none(), "should omit 'plugins'");
    assert!(json.get("files").is_some(), "should include 'files'");
    assert!(
        json.get("file_count").is_some(),
        "should include 'file_count'"
    );
    assert!(
        json.get("entry_points").is_none(),
        "should omit 'entry_points'"
    );
}

#[test]
fn list_entry_points_only_json_omits_plugins_and_files() {
    let output = run_list("basic-project", &["--entry-points", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    assert!(json.get("plugins").is_none(), "should omit 'plugins'");
    assert!(json.get("files").is_none(), "should omit 'files'");
    assert!(
        json.get("entry_points").is_some(),
        "should include 'entry_points'"
    );
    assert!(
        json.get("entry_point_count").is_some(),
        "should include 'entry_point_count'"
    );
}

#[test]
fn list_show_all_json_omits_boundaries_even_when_configured() {
    let output = run_list("boundary-violations", &["--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    assert!(
        json.get("boundaries").is_none(),
        "show_all mode should not include boundaries without --boundaries"
    );
    assert!(
        json.get("files").is_some(),
        "show_all mode should still include files"
    );
    assert!(
        json.get("entry_points").is_some(),
        "show_all mode should still include entry points"
    );
}

#[test]
fn list_boundaries_only_json_omits_plugins_files_and_entry_points() {
    let output = run_list("boundary-violations", &["--boundaries", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    assert!(json.get("plugins").is_none(), "should omit 'plugins'");
    assert!(json.get("files").is_none(), "should omit 'files'");
    assert!(
        json.get("entry_points").is_none(),
        "should omit 'entry_points'"
    );
    assert!(
        json.get("boundaries").is_some(),
        "should include 'boundaries'"
    );
}

#[test]
fn list_json_files_are_relative_paths() {
    let output = run_list("basic-project", &["--files", "--format", "json"]);
    let json = parse_json(&output);

    let files = json["files"].as_array().unwrap();
    for file in files {
        let path = file.as_str().unwrap();
        assert!(
            !path.starts_with('/'),
            "file path should be relative, got: {path}"
        );
        assert!(
            !path.contains('\\'),
            "file path should use forward slashes, got: {path}"
        );
        assert!(
            path.starts_with("src/"),
            "file path should start with src/, got: {path}"
        );
    }
}

#[test]
fn list_json_entry_point_paths_are_relative() {
    let output = run_list("basic-project", &["--entry-points", "--format", "json"]);
    let json = parse_json(&output);

    let eps = json["entry_points"].as_array().unwrap();
    for ep in eps {
        let path = ep["path"].as_str().unwrap();
        assert!(
            !path.starts_with('/'),
            "entry point path should be relative, got: {path}"
        );
        assert!(
            !path.contains('\\'),
            "entry point path should use forward slashes, got: {path}"
        );
    }
}

#[test]
fn list_basic_project_detects_typescript_plugin() {
    let output = run_list("basic-project", &["--plugins", "--format", "json"]);
    let json = parse_json(&output);

    let plugins = json["plugins"].as_array().unwrap();
    let names: Vec<&str> = plugins
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"typescript"),
        "basic-project has typescript devDep, should detect typescript plugin. Got: {names:?}"
    );
}

#[test]
fn list_nextjs_project_detects_nextjs_plugin() {
    let output = run_list("nextjs-project", &["--plugins", "--format", "json"]);
    let json = parse_json(&output);

    let plugins = json["plugins"].as_array().unwrap();
    let names: Vec<&str> = plugins
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"nextjs"),
        "nextjs-project should detect nextjs plugin. Got: {names:?}"
    );
}

#[test]
fn list_external_plugin_detected() {
    let output = run_list("external-plugins", &["--plugins", "--format", "json"]);
    let json = parse_json(&output);

    let plugins = json["plugins"].as_array().unwrap();
    let names: Vec<&str> = plugins
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"my-framework"),
        "external-plugins fixture should detect my-framework plugin. Got: {names:?}"
    );
}

#[test]
fn list_entry_point_has_source_field() {
    let output = run_list("basic-project", &["--entry-points", "--format", "json"]);
    let json = parse_json(&output);

    let eps = json["entry_points"].as_array().unwrap();
    assert!(!eps.is_empty(), "basic-project should have entry points");

    for ep in eps {
        assert!(ep.get("path").is_some(), "entry point missing 'path' field");
        assert!(
            ep.get("source").is_some(),
            "entry point missing 'source' field"
        );
        let source = ep["source"].as_str().unwrap();
        assert!(!source.is_empty(), "entry point source should not be empty");
    }
}

#[test]
fn list_basic_project_main_entry_point_source() {
    let output = run_list("basic-project", &["--entry-points", "--format", "json"]);
    let json = parse_json(&output);

    let eps = json["entry_points"].as_array().unwrap();
    let main_ep = eps
        .iter()
        .find(|ep| {
            let p = ep["path"].as_str().unwrap();
            p == "src/index.ts" || p == "src\\index.ts"
        })
        .expect("should have src/index.ts as entry point");

    assert_eq!(
        main_ep["source"].as_str().unwrap(),
        "package.json main",
        "src/index.ts should be detected via package.json main"
    );
}

#[test]
fn list_plugin_discovered_entry_points_in_show_all_mode() {
    let output = run_list("external-plugins", &["--format", "json"]);
    let json = parse_json(&output);

    let eps = json["entry_points"].as_array().unwrap();
    let plugin_eps: Vec<&serde_json::Value> = eps
        .iter()
        .filter(|ep| ep["source"].as_str().is_some_and(|s| s == "my-framework"))
        .collect();

    assert!(
        !plugin_eps.is_empty(),
        "external-plugins should have plugin-discovered entry points in show_all mode"
    );

    for ep in &plugin_eps {
        let source = ep["source"].as_str().unwrap();
        assert_eq!(
            source, "my-framework",
            "plugin entry point source should be 'my-framework', got: {source}"
        );
    }
}

#[test]
fn list_entry_points_only_includes_plugin_entries() {
    let all_output = run_list("external-plugins", &["--format", "json"]);
    let all_json = parse_json(&all_output);
    let all_eps = all_json["entry_points"].as_array().unwrap();

    let ep_output = run_list("external-plugins", &["--entry-points", "--format", "json"]);
    let ep_json = parse_json(&ep_output);
    let ep_only = ep_json["entry_points"].as_array().unwrap();

    assert!(
        ep_only
            .iter()
            .any(|ep| ep["source"].as_str().is_some_and(|s| s == "my-framework")),
        "--entry-points output should include plugin-discovered entry points",
    );
    assert_eq!(
        all_eps.len(),
        ep_only.len(),
        "show_all mode ({}) and --entry-points only mode ({}) should report the same entry points",
        all_eps.len(),
        ep_only.len(),
    );
}

#[test]
fn list_workspace_project_discovers_files_across_packages() {
    let output = run_list("workspace-project", &["--files", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    let files = json["files"].as_array().unwrap();

    let has_app = files.iter().any(|f| {
        let p = f.as_str().unwrap();
        p.starts_with("packages/app/") || p.starts_with("packages\\app\\")
    });
    let has_shared = files.iter().any(|f| {
        let p = f.as_str().unwrap();
        p.starts_with("packages/shared/") || p.starts_with("packages\\shared\\")
    });
    let has_utils = files.iter().any(|f| {
        let p = f.as_str().unwrap();
        p.starts_with("packages/utils/") || p.starts_with("packages\\utils\\")
    });

    assert!(has_app, "should discover files in packages/app/");
    assert!(has_shared, "should discover files in packages/shared/");
    assert!(has_utils, "should discover files in packages/utils/");
}

#[test]
fn list_workspace_project_discovers_entry_points_from_multiple_packages() {
    let output = run_list("workspace-project", &["--entry-points", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    let eps = json["entry_points"].as_array().unwrap();

    let app_entries = eps
        .iter()
        .filter(|ep| {
            let p = ep["path"].as_str().unwrap();
            p.starts_with("packages/app/") || p.starts_with("packages\\app\\")
        })
        .count();
    let shared_entries = eps
        .iter()
        .filter(|ep| {
            let p = ep["path"].as_str().unwrap();
            p.starts_with("packages/shared/") || p.starts_with("packages\\shared\\")
        })
        .count();

    assert!(
        app_entries > 0,
        "should have entry points from packages/app/"
    );
    assert!(
        shared_entries > 0,
        "should have entry points from packages/shared/"
    );
}

#[test]
fn list_boundaries_json_reports_zone_and_rule_counts() {
    let output = run_list("boundary-violations", &["--boundaries", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    let boundaries = &json["boundaries"];

    assert_eq!(
        boundaries["configured"].as_bool(),
        Some(true),
        "boundary fixture should report configured=true"
    );
    assert_eq!(
        boundaries["zone_count"].as_u64(),
        Some(3),
        "boundary fixture should expose 3 zones"
    );
    assert_eq!(
        boundaries["rule_count"].as_u64(),
        Some(2),
        "boundary fixture should expose 2 rules"
    );

    let zones = boundaries["zones"].as_array().unwrap();
    let ui_zone = zones
        .iter()
        .find(|zone| zone["name"].as_str() == Some("ui"))
        .expect("should include ui zone");
    assert_eq!(
        ui_zone["file_count"].as_u64(),
        Some(1),
        "ui zone should match one file in the fixture"
    );
}

#[test]
fn list_boundaries_json_reports_not_configured_when_absent() {
    let output = run_list("basic-project", &["--boundaries", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    let boundaries = &json["boundaries"];

    assert_eq!(
        boundaries["configured"].as_bool(),
        Some(false),
        "projects without boundaries should report configured=false"
    );
    assert_eq!(
        boundaries["zones"].as_array().map(std::vec::Vec::len),
        Some(0),
        "projects without boundaries should expose an empty zones array"
    );
    assert_eq!(
        boundaries["rules"].as_array().map(std::vec::Vec::len),
        Some(0),
        "projects without boundaries should expose an empty rules array"
    );
}

#[test]
fn list_human_output_plugins_section() {
    let output = run_list("basic-project", &["--plugins"]);
    assert_eq!(output.code, 0);

    assert!(
        output.stderr.contains("Active plugins:"),
        "human output should contain 'Active plugins:' header in stderr. Got stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("typescript"),
        "human output should list typescript plugin in stderr. Got stderr: {}",
        output.stderr
    );
    assert!(
        output.stdout.trim().is_empty(),
        "stdout should be empty for --plugins in human format. Got: {}",
        output.stdout
    );
}

#[test]
fn list_human_output_files_section() {
    let output = run_list("basic-project", &["--files"]);
    assert_eq!(output.code, 0);

    assert!(
        output.stderr.contains("Discovered"),
        "human output should say 'Discovered' in stderr. Got stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("files"),
        "human output should mention 'files' in stderr. Got stderr: {}",
        output.stderr
    );

    assert!(
        output.stdout.contains("index.ts"),
        "human output stdout should list index.ts. Got: {}",
        output.stdout
    );
}

#[test]
fn list_human_output_entry_points_section() {
    let output = run_list("basic-project", &["--entry-points"]);
    assert_eq!(output.code, 0);

    assert!(
        output.stderr.contains("Found"),
        "human output should say 'Found' in stderr. Got stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("entry points"),
        "human output should mention 'entry points' in stderr. Got stderr: {}",
        output.stderr
    );

    assert!(
        output.stdout.contains("index.ts"),
        "human output stdout should list entry point path. Got: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains("package.json main"),
        "human output should include entry point source. Got: {}",
        output.stdout
    );
}

#[test]
fn list_human_show_all_omits_boundaries_when_not_requested() {
    let output = run_list("boundary-violations", &[]);
    assert_eq!(output.code, 0);

    assert!(
        !output.stderr.contains("Boundaries:"),
        "show_all human output should omit boundaries without --boundaries. Got stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("Discovered"),
        "show_all human output should still include the files section. Got stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("Found"),
        "show_all human output should still include the entry points section. Got stderr: {}",
        output.stderr
    );
}

#[test]
fn list_human_output_boundaries_section() {
    let output = run_list("boundary-violations", &["--boundaries"]);
    assert_eq!(output.code, 0);

    assert!(
        output.stderr.contains("Boundaries: 3 zones, 2 rules"),
        "human output should summarize configured boundaries. Got stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("Zones:"),
        "human output should include a zones section. Got stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("Rules:"),
        "human output should include a rules section. Got stderr: {}",
        output.stderr
    );
}

#[test]
fn list_human_output_files_are_relative_paths() {
    let output = run_list("basic-project", &["--files"]);

    for line in output.stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        assert!(
            !trimmed.starts_with('/')
                && !trimmed.starts_with("\\\\")
                && trimmed.chars().nth(1) != Some(':'),
            "human output file path should be relative, got: {trimmed}"
        );
    }
}

#[test]
fn list_json_plugins_array_items_have_name_field() {
    let output = run_list("basic-project", &["--plugins", "--format", "json"]);
    let json = parse_json(&output);

    let plugins = json["plugins"].as_array().unwrap();
    for plugin in plugins {
        assert!(
            plugin.get("name").is_some(),
            "each plugin object should have a 'name' field"
        );
        assert!(
            plugin["name"].is_string(),
            "plugin 'name' should be a string"
        );
    }
}

#[test]
fn list_json_entry_points_array_items_have_path_and_source() {
    let output = run_list("basic-project", &["--entry-points", "--format", "json"]);
    let json = parse_json(&output);

    let eps = json["entry_points"].as_array().unwrap();
    for ep in eps {
        assert!(ep.get("path").is_some(), "entry point should have 'path'");
        assert!(
            ep.get("source").is_some(),
            "entry point should have 'source'"
        );
        assert!(ep["path"].is_string(), "'path' should be a string");
        assert!(ep["source"].is_string(), "'source' should be a string");
    }
}

#[test]
fn list_json_files_are_sorted_alphabetically() {
    let output = run_list("basic-project", &["--files", "--format", "json"]);
    let json = parse_json(&output);

    let files: Vec<&str> = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();

    let mut sorted = files.clone();
    sorted.sort_unstable();
    assert_eq!(files, sorted, "files should be in sorted order");
}

#[test]
fn list_plugins_and_files_together_json() {
    let output = run_list(
        "basic-project",
        &["--plugins", "--files", "--format", "json"],
    );
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    assert!(json.get("plugins").is_some(), "should include 'plugins'");
    assert!(json.get("files").is_some(), "should include 'files'");
    assert!(
        json.get("entry_points").is_none(),
        "should omit 'entry_points' when only --plugins --files"
    );
}

#[test]
fn list_files_and_entry_points_together_json() {
    let output = run_list(
        "basic-project",
        &["--files", "--entry-points", "--format", "json"],
    );
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    assert!(json.get("plugins").is_none(), "should omit 'plugins'");
    assert!(json.get("files").is_some(), "should include 'files'");
    assert!(
        json.get("entry_points").is_some(),
        "should include 'entry_points'"
    );
}

#[test]
fn list_returns_exit_code_0_on_success() {
    let output = run_list("basic-project", &["--format", "json"]);
    assert_eq!(
        output.code, 0,
        "list command should always return exit code 0 on success"
    );
}

#[test]
fn combined_json_errors_on_invalid_plugin_regex() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_project_with_invalid_tanstack_route_ignore_pattern(dir.path());

    let output = run_fallow_combined_in_root(dir.path(), &["--format", "json", "--quiet"]);

    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["error"], serde_json::Value::Bool(true));
    assert_eq!(json["exit_code"], serde_json::Value::from(2));
    let message = json["message"]
        .as_str()
        .expect("message should be a string");
    assert!(
        message.contains("invalid plugin regex configuration"),
        "message: {message}"
    );
    assert!(message.contains("tanstack-router"), "message: {message}");
    assert!(
        message.contains("entry_patterns[].exclude_segment_regexes"),
        "message: {message}"
    );
    assert!(
        message.contains("Rewrite the plugin config with Rust-compatible regex syntax"),
        "message: {message}"
    );
    assert!(message.contains("vite.config.ts"), "message: {message}");
    assert!(
        !message.contains("future release"),
        "message should not include old warning tail: {message}"
    );
}

#[test]
fn list_plugins_json_errors_on_invalid_plugin_regex() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_project_with_invalid_tanstack_route_ignore_pattern(dir.path());

    let output = run_fallow_combined_in_root(
        dir.path(),
        &["list", "--plugins", "--format", "json", "--quiet"],
    );

    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["error"], serde_json::Value::Bool(true));
    assert_eq!(json["exit_code"], serde_json::Value::from(2));
    let message = json["message"]
        .as_str()
        .expect("message should be a string");
    assert!(
        message.contains("invalid plugin regex configuration"),
        "message: {message}"
    );
    assert!(message.contains("tanstack-router"), "message: {message}");
    assert!(
        message.contains("entry_patterns[].exclude_segment_regexes"),
        "message: {message}"
    );
    assert!(
        message.contains("Rewrite the plugin config with Rust-compatible regex syntax"),
        "message: {message}"
    );
}

#[test]
fn list_cjs_project_discovers_js_files() {
    let output = run_list("cjs-project", &["--files", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    let files: Vec<&str> = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();

    assert!(
        files.iter().any(|f| {
            std::path::Path::new(f)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("js"))
        }),
        "cjs-project should discover .js files. Got: {files:?}"
    );
}

#[test]
fn list_vue_project_discovers_vue_files() {
    let output = run_list("vue-project", &["--files", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    let files: Vec<&str> = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();

    assert!(
        files.iter().any(|f| {
            std::path::Path::new(f)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("vue"))
        }),
        "vue-project should discover .vue files. Got: {files:?}"
    );
}

#[test]
fn list_svelte_project_discovers_svelte_files() {
    let output = run_list("svelte-project", &["--files", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    let files: Vec<&str> = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();

    assert!(
        files.iter().any(|f| f.ends_with(".svelte")),
        "svelte-project should discover .svelte files. Got: {files:?}"
    );
}

#[test]
fn list_css_modules_project_discovers_css_module_files() {
    let output = run_list("css-modules-project", &["--files", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    let files: Vec<&str> = json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();

    assert!(
        files.iter().any(|f| f.contains(".module.css")),
        "css-modules-project should discover .module.css files. Got: {files:?}"
    );
}

#[test]
fn list_production_mode_flag_accepted() {
    let output = run_list(
        "basic-project",
        &["--production", "--files", "--format", "json"],
    );
    assert_eq!(output.code, 0, "list with --production should succeed");

    let json = parse_json(&output);
    assert!(
        json.get("files").is_some(),
        "should still list files in production mode"
    );
}

#[test]
fn list_invalid_root_returns_error() {
    let bin = fallow_bin();
    let output = Command::new(&bin)
        .arg("list")
        .arg("--root")
        .arg("/nonexistent/path/that/does/not/exist")
        .env("RUST_LOG", "")
        .output()
        .expect("failed to run fallow binary");

    assert_ne!(
        output.status.code().unwrap_or(0),
        0,
        "should return non-zero exit code for invalid root"
    );
}

#[test]
fn list_json_output_is_valid_json_object() {
    let output = run_list("basic-project", &["--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    assert!(json.is_object(), "JSON output should be an object");
}

#[test]
fn list_project_without_known_plugins_has_empty_or_minimal_plugins() {
    let output = run_list("detect-config", &["--plugins", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    let plugins = json["plugins"].as_array();
    assert!(
        plugins.is_some(),
        "plugins should always be an array, even if empty-ish"
    );
}

#[test]
fn list_workspace_project_entry_points_have_varied_sources() {
    let output = run_list("workspace-project", &["--entry-points", "--format", "json"]);
    assert_eq!(output.code, 0);

    let json = parse_json(&output);
    let eps = json["entry_points"].as_array().unwrap();
    let sources: Vec<&str> = eps
        .iter()
        .map(|ep| ep["source"].as_str().unwrap())
        .collect();

    assert!(
        sources.len() > 1,
        "workspace-project should have multiple entry points. Got: {sources:?}"
    );
}

#[test]
fn list_nextjs_project_app_page_is_plugin_entry_point() {
    let output = run_list("nextjs-project", &["--format", "json"]);
    let json = parse_json(&output);

    let eps = json["entry_points"].as_array().unwrap();
    let page_ep = eps
        .iter()
        .find(|ep| ep["path"].as_str().unwrap().contains("page.tsx"));

    assert!(
        page_ep.is_some(),
        "nextjs-project should have page.tsx as entry point"
    );

    let source = page_ep.unwrap()["source"].as_str().unwrap();
    assert_eq!(
        source, "nextjs",
        "page.tsx should be discovered by nextjs plugin. Got source: {source}"
    );
}

#[test]
fn list_files_includes_plugin_scoped_hidden_dirs_for_react_router() {
    let output = run_list("react-router-conventions", &["--files", "--format", "json"]);
    assert_eq!(output.code, 0, "stderr was: {}", output.stderr);

    let json = parse_json(&output);
    let files: Vec<&str> = json["files"]
        .as_array()
        .expect("files array")
        .iter()
        .map(|v| v.as_str().expect("file path string"))
        .collect();

    assert!(
        files.contains(&"app/.client/analytics.ts"),
        "expected app/.client/analytics.ts in files: {files:?}"
    );
    assert!(
        files.contains(&"app/.server/db.ts"),
        "expected app/.server/db.ts in files: {files:?}"
    );
}

#[test]
fn list_files_includes_plugin_scoped_hidden_dirs_for_remix() {
    let output = run_list("remix-conventions", &["--files", "--format", "json"]);
    assert_eq!(output.code, 0, "stderr was: {}", output.stderr);

    let json = parse_json(&output);
    let files: Vec<&str> = json["files"]
        .as_array()
        .expect("files array")
        .iter()
        .map(|v| v.as_str().expect("file path string"))
        .collect();

    assert!(
        files.contains(&"app/.client/analytics.ts"),
        "expected app/.client/analytics.ts in files: {files:?}"
    );
    assert!(
        files.contains(&"app/.server/db.ts"),
        "expected app/.server/db.ts in files: {files:?}"
    );
}

/// Issue #2366 follow-up: the `fallow workspaces` / `fallow list --workspaces`
/// envelope has no post-serialization root-prefix strip, so its
/// `workspace_diagnostics[].path` used to be the only absolute path in any
/// fallow JSON envelope while the `workspaces[].path` next to it was relative.
#[test]
fn list_workspaces_json_emits_project_relative_diagnostic_paths() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("packages/inner/src")).expect("create inner package dir");
    fs::create_dir_all(root.join("src")).expect("create source dir");
    fs::write(
        root.join("package.json"),
        r#"{"name":"undeclared-workspace-root","private":true,"workspaces":["packages/declared"]}"#,
    )
    .expect("write root manifest");
    fs::write(
        root.join("packages/inner/package.json"),
        r#"{"name":"inner-pkg","version":"1.0.0"}"#,
    )
    .expect("write inner manifest");
    fs::write(root.join("src/index.ts"), "export const value = 1;\n").expect("write source");
    fs::write(
        root.join("packages/inner/src/index.ts"),
        "export const inner = 2;\n",
    )
    .expect("write inner source");

    for subcommand in ["list", "workspaces"] {
        let args: Vec<&str> = if subcommand == "list" {
            vec!["list", "--workspaces", "--format", "json", "--quiet"]
        } else {
            vec!["workspaces", "--format", "json", "--quiet"]
        };
        let output = run_fallow_combined_in_root(root, &args);
        assert_eq!(output.code, 0, "stderr: {}", output.stderr);

        let json = parse_json(&output);
        let diagnostics = json["workspace_diagnostics"]
            .as_array()
            .expect("workspace_diagnostics array");
        let path = diagnostics
            .iter()
            .find(|entry| entry["kind"] == "undeclared-workspace")
            .expect("the undeclared workspace is reported")["path"]
            .as_str()
            .expect("diagnostic path string");
        assert_eq!(
            path, "packages/inner",
            "`fallow {subcommand}` must emit a project-relative diagnostic path, got {path}"
        );
    }
}

/// Issue #2366: bare `fallow list --format json` reads the engine session's
/// diagnostics snapshot, whose fold is keyed on the whole diagnostic kind
/// rather than its id. Two overlapping workspace globs report the same
/// package-less directory once per `pattern`, and bare `list` used to collapse
/// them into one while `list --workspaces`, which reads the workspace value
/// directly, reported both. Pin the agreement between the two.
#[test]
fn list_json_keeps_both_overlapping_glob_diagnostics() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("pkgs/aaa")).expect("create package-less dir");
    fs::create_dir_all(root.join("src")).expect("create source dir");
    fs::write(
        root.join("package.json"),
        r#"{"name":"overlapping-glob-root","private":true,"workspaces":["pkgs/*","pkgs/a*"]}"#,
    )
    .expect("write root manifest");
    fs::write(root.join("src/index.ts"), "export const value = 1;\n").expect("write source");
    fs::write(root.join("pkgs/aaa/readme.txt"), "no package.json here\n").expect("write filler");

    for args in [
        ["list", "--format", "json", "--quiet"].as_slice(),
        ["list", "--workspaces", "--format", "json", "--quiet"].as_slice(),
    ] {
        let output = run_fallow_combined_in_root(root, args);
        assert_eq!(output.code, 0, "stderr: {}", output.stderr);

        let json = parse_json(&output);
        let patterns: Vec<String> = json["workspace_diagnostics"]
            .as_array()
            .map(|diagnostics| {
                diagnostics
                    .iter()
                    .filter(|entry| entry["kind"] == "glob-matched-no-package-json")
                    .map(|entry| entry["pattern"].as_str().unwrap_or_default().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            patterns,
            ["pkgs/*", "pkgs/a*"],
            "`fallow {args:?}` reports the directory once per matching glob: {}",
            json["workspace_diagnostics"]
        );
    }
}

/// Build the issue-2366 repository shape: one glob declared in both
/// `package.json` (spelled `./pkgs/*`) and `pnpm-workspace.yaml` (spelled
/// `pkgs/*`), over two directories that carry no `package.json`.
fn write_two_manifest_glob_project(root: &std::path::Path) {
    fs::create_dir_all(root.join("pkgs/aaa")).expect("create first package-less dir");
    fs::create_dir_all(root.join("pkgs/bbb")).expect("create second package-less dir");
    fs::create_dir_all(root.join("src")).expect("create source dir");
    fs::write(
        root.join("package.json"),
        r#"{"name":"two-manifest-root","private":true,"workspaces":["./pkgs/*"]}"#,
    )
    .expect("write root manifest");
    fs::write(
        root.join("pnpm-workspace.yaml"),
        "packages:\n  - \"pkgs/*\"\n",
    )
    .expect("write pnpm workspace manifest");
    fs::write(root.join("src/index.ts"), "export const value = 1;\n").expect("write source");
    fs::write(root.join("pkgs/aaa/readme.txt"), "no package.json here\n").expect("write filler");
    fs::write(root.join("pkgs/bbb/readme.txt"), "no package.json here\n").expect("write filler");
}

/// Issue #2366: `package.json` `workspaces` and `pnpm-workspace.yaml`
/// `packages` are additive, so one glob declared in both is walked twice.
/// Every envelope that carries `workspace_diagnostics[]` must report one entry
/// per distinct matching pattern, with the same project-relative path shape,
/// whichever manifest happened to be read first.
#[test]
fn every_envelope_reports_one_entry_per_directory_for_a_glob_in_two_manifests() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    write_two_manifest_glob_project(root);

    for args in [
        ["--format", "json", "--quiet"].as_slice(),
        ["--skip", "check", "--format", "json", "--quiet"].as_slice(),
        ["--only", "health", "--format", "json", "--quiet"].as_slice(),
        ["dead-code", "--format", "json", "--quiet"].as_slice(),
        ["check", "--format", "json", "--quiet"].as_slice(),
        ["health", "--format", "json", "--quiet"].as_slice(),
        ["dupes", "--format", "json", "--quiet"].as_slice(),
        ["list", "--format", "json", "--quiet"].as_slice(),
        ["list", "--workspaces", "--format", "json", "--quiet"].as_slice(),
        ["workspaces", "--format", "json", "--quiet"].as_slice(),
    ] {
        let output = run_fallow_combined_in_root(root, args);
        assert_eq!(output.code, 0, "stderr: {}", output.stderr);

        let json = parse_json(&output);
        // Restricted to the glob kind: a bare fixture also reports environment
        // and unconfigured-detector kinds, which carry no pattern and say
        // nothing about per-directory deduplication.
        let reported: Vec<(String, String)> = json["workspace_diagnostics"]
            .as_array()
            .expect("workspace_diagnostics array")
            .iter()
            .filter(|entry| entry["kind"] == "glob-matched-no-package-json")
            .map(|entry| {
                (
                    entry["pattern"].as_str().unwrap_or_default().to_owned(),
                    entry["path"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            reported,
            vec![
                ("pkgs/*".to_owned(), "pkgs/aaa".to_owned()),
                ("pkgs/*".to_owned(), "pkgs/bbb".to_owned()),
            ],
            "`fallow {args:?}` reports each directory once, project-relative: {}",
            json["workspace_diagnostics"]
        );
    }
}

/// The aggregated stderr warning is built from the same list, so a duplicated
/// entry makes it claim a directory count the repository does not have and
/// name one directory twice among its examples.
#[test]
fn two_manifest_glob_warning_names_the_true_directory_count_once_each() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    write_two_manifest_glob_project(root);

    let output = Command::new(fallow_bin())
        .arg("--root")
        .arg(root)
        .args(["workspaces", "--format", "json"])
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run fallow binary");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let warnings: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("no package.json"))
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "one glob is one summary line, whichever manifests declare it: {stderr}"
    );
    assert!(
        warnings[0].contains(
            "Glob 'pkgs/*' matched 2 directories with no package.json \
             (e.g. pkgs/aaa, pkgs/bbb)"
        ),
        "the summary counts the directories once each: {}",
        warnings[0]
    );
}

/// The human workspace listing renders the same list as the JSON envelope, so
/// the deduplication moves it too: its header counts the entries and its body
/// prints one line each. This is the non-JSON surface the change reaches, and
/// the CHANGELOG names it next to the aggregated warning.
#[test]
fn two_manifest_glob_human_listing_prints_each_directory_once() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    write_two_manifest_glob_project(root);

    let output = run_fallow_combined_in_root(root, &["workspaces"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);

    assert!(
        output.stderr.contains("2 workspace discovery diagnostics:"),
        "the block header counts the deduplicated entries: {}",
        output.stderr
    );
    let entries: Vec<&str> = output
        .stderr
        .lines()
        .filter(|line| line.trim_start().starts_with("- Glob "))
        .collect();
    assert_eq!(
        entries.len(),
        2,
        "one line per directory, not one per manifest that declares the glob: {}",
        output.stderr
    );
    for (entry, directory) in entries.iter().zip(["pkgs/aaa", "pkgs/bbb"]) {
        assert!(
            entry.contains(&format!("Glob 'pkgs/*' matched '{directory}'")),
            "the block quotes the canonical glob spelling: {entry}"
        );
    }
}

/// Issue #2366: the `./` normalisation is independent of the deduplication.
/// A repository that declares one glob, once, in one manifest still reports a
/// different `pattern`, `path`, and `message` than before when that glob is
/// spelled with a leading `./`, on the standalone envelopes as well. The
/// CHANGELOG names this as one of the two shapes that move them, so pin it
/// separately from the two-manifest fixture where the fold is also at work.
#[test]
fn a_dotted_glob_declared_once_reports_the_undotted_spelling_everywhere() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("pkgs/aaa")).expect("create package-less dir");
    fs::create_dir_all(root.join("src")).expect("create source dir");
    fs::write(
        root.join("package.json"),
        r#"{"name":"dotted-glob-root","private":true,"workspaces":["./pkgs/*"]}"#,
    )
    .expect("write root manifest");
    fs::write(root.join("src/index.ts"), "export const value = 1;\n").expect("write source");
    fs::write(root.join("pkgs/aaa/readme.txt"), "no package.json here\n").expect("write filler");

    for args in [
        ["dead-code", "--format", "json", "--quiet"].as_slice(),
        ["check", "--format", "json", "--quiet"].as_slice(),
        ["health", "--format", "json", "--quiet"].as_slice(),
        ["dupes", "--format", "json", "--quiet"].as_slice(),
        ["list", "--format", "json", "--quiet"].as_slice(),
        ["list", "--workspaces", "--format", "json", "--quiet"].as_slice(),
        ["workspaces", "--format", "json", "--quiet"].as_slice(),
        ["--format", "json", "--quiet"].as_slice(),
    ] {
        let output = run_fallow_combined_in_root(root, args);
        assert_eq!(output.code, 0, "stderr: {}", output.stderr);

        let json = parse_json(&output);
        // Only the glob diagnostics are the subject here. A bare fixture also
        // reports environment and unconfigured-detector kinds, which carry no
        // pattern and are a separate channel.
        let diagnostics: Vec<&serde_json::Value> = json["workspace_diagnostics"]
            .as_array()
            .expect("workspace_diagnostics array")
            .iter()
            .filter(|entry| entry["kind"] == "glob-matched-no-package-json")
            .collect();
        assert_eq!(
            diagnostics.len(),
            1,
            "`fallow {args:?}` reports the one matched directory once: {}",
            json["workspace_diagnostics"]
        );
        assert_eq!(
            diagnostics[0]["pattern"], "pkgs/*",
            "`fallow {args:?}` drops the no-op `./` prefix from the pattern"
        );
        assert_eq!(
            diagnostics[0]["path"], "pkgs/aaa",
            "`fallow {args:?}` drops the matching no-op `.` component from the path"
        );
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .unwrap_or_default()
                .starts_with("Glob 'pkgs/*' matched 'pkgs/aaa'"),
            "`fallow {args:?}` quotes the canonical spelling in the message: {}",
            diagnostics[0]["message"]
        );
    }
}

/// `list` loads no baseline and saves none. The global baseline flags must
/// fail with exit 2 and a structured error instead of a silent exit 0.
#[test]
fn list_rejects_global_baseline_flags() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    fs::write(root.join("package.json"), r#"{"name": "list-baseline"}"#)
        .expect("write package json");
    let target = root.join("out.json");
    for flag in ["--baseline", "--save-baseline"] {
        let output = crate::common::run_fallow_in_root(
            "list",
            root,
            &[
                flag,
                target.to_str().unwrap(),
                "--format",
                "json",
                "--quiet",
            ],
        );
        assert_eq!(
            output.code, 2,
            "list {flag} should exit 2. stdout: {} stderr: {}",
            output.stdout, output.stderr
        );
        let doc = parse_json(&output);
        assert_eq!(doc["error"], true, "list {flag}: {doc}");
        let message = doc["message"].as_str().unwrap_or_default();
        assert!(message.contains("`fallow list`"), "{message}");
        assert!(message.contains(flag), "{message}");
        for supported in ["`fallow dead-code`", "`fallow dupes`", "`fallow health`"] {
            assert!(
                message.contains(supported),
                "list {flag}: message names {supported}: {message}"
            );
        }
        assert!(!target.exists(), "list {flag} must not write a file");
    }
}

/// `fallow list` uses the plugin stage and the entry-point discovery of the
/// analysis, auto-import gate included, so `--entry-points` names the entry
/// points the analysis uses (issue #2804). With `autoImports: true`, Nuxt components and composables
/// are reached through their auto-imports, not as entry points, and the
/// analysis reports the unreferenced ones as unused files. The fixture sets
/// `autoImports: true` explicitly: without it, the two paths agree anyway.
#[test]
fn list_entry_points_match_the_analysis_under_auto_imports() {
    let listed = parse_json(&run_list(
        "list-nuxt-auto-imports-gate",
        &["--entry-points", "--format", "json", "--quiet"],
    ));
    let paths: Vec<&str> = listed["entry_points"]
        .as_array()
        .expect("entry_points array")
        .iter()
        .filter_map(|entry| entry["path"].as_str())
        .collect();
    assert!(
        !paths.contains(&"app/components/DeadCard.vue")
            && !paths.contains(&"app/composables/useDead.ts"),
        "the analysis reports these files as unused, so list must not name them as entry points: {paths:?}"
    );

    let analysis = parse_json(&run_fallow(
        "dead-code",
        "list-nuxt-auto-imports-gate",
        &["--format", "json", "--quiet"],
    ));
    assert_eq!(
        listed["entry_point_count"], analysis["entry_points"]["total"],
        "list and the analysis must count the same entry points: {paths:?}"
    );
}

/// The plugin stage records `plugin-effect-not-modeled` when it cannot read a
/// Nuxt config key. The analysis envelopes carry it; `fallow list` now does
/// too, in its own `workspace_diagnostics`, including on an entry-point-only
/// listing where the plugin stage ran.
#[test]
fn list_records_the_plugin_diagnostics_the_analysis_records() {
    let analysis = parse_json(&run_fallow(
        "dead-code",
        "list-nuxt-auto-imports-unreadable",
        &["--format", "json", "--quiet"],
    ));
    let plugin_kinds = |json: &serde_json::Value| -> Vec<String> {
        json["workspace_diagnostics"]
            .as_array()
            .map(|diagnostics| {
                diagnostics
                    .iter()
                    .filter_map(|diagnostic| diagnostic["kind"].as_str())
                    .filter(|kind| kind.starts_with("plugin-"))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let expected = plugin_kinds(&analysis);
    assert_eq!(
        expected,
        vec!["plugin-effect-not-modeled", "plugin-effect-not-modeled"],
        "the fixture must reproduce the analysis-side diagnostics"
    );

    for args in [
        vec!["--format", "json", "--quiet"],
        vec!["--entry-points", "--format", "json", "--quiet"],
    ] {
        let listed = parse_json(&run_list("list-nuxt-auto-imports-unreadable", &args));
        assert_eq!(
            plugin_kinds(&listed),
            expected,
            "`list {args:?}` must record what the analysis recorded: {}",
            listed["workspace_diagnostics"]
        );
    }
}

// ---------------------------------------------------------------------------
// --entry-weight
// ---------------------------------------------------------------------------

const ENTRY_WEIGHT_FIXTURE: &str = "startup-import-weight";

fn fixture_file_bytes(paths: &[&str]) -> u64 {
    let root = crate::common::fixture_path(ENTRY_WEIGHT_FIXTURE);
    paths
        .iter()
        .map(|path| fs::metadata(root.join(path)).expect("fixture file").len())
        .sum()
}

#[test]
fn list_entry_weight_json_reports_eager_deferred_and_out_of_thread_weight() {
    let output = run_list(
        ENTRY_WEIGHT_FIXTURE,
        &["--entry-weight", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);

    assert!(
        json.get("entry_points").is_none(),
        "--entry-weight alone does not list the entry points"
    );
    let weight = &json["entry_weight"];
    assert_eq!(weight["unit"], "source_bytes");
    assert_eq!(weight["entry_count"], 1);
    let entry = &weight["entries"][0];
    assert_eq!(entry["path"], "src/index.ts");
    assert_eq!(entry["eager_modules"], 9);
    assert_eq!(
        entry["eager_bytes"],
        fixture_file_bytes(&[
            "src/index.ts",
            "src/heavy/view.ts",
            "src/heavy/chart-data.ts",
            "src/heavy/formatters.ts",
            "src/shared.ts",
            "src/styles.css",
            "src/reexported.ts",
            "src/legacy.js",
            "src/eager/one.ts",
        ])
    );
    assert_eq!(
        entry["eager_css_bytes"],
        fixture_file_bytes(&["src/styles.css"])
    );
    assert_eq!(entry["deferred_modules"], 4);
    assert_eq!(
        entry["deferred_bytes"],
        fixture_file_bytes(&[
            "src/lazy.ts",
            "src/lazy-only.ts",
            "src/pages/home.ts",
            "src/lazy-glob/two.ts",
        ])
    );
    assert_eq!(entry["out_of_thread_modules"], 3);
    assert_eq!(
        entry["out_of_thread_bytes"],
        fixture_file_bytes(&["src/worker.ts", "src/worker-only.ts", "src/child.js"])
    );

    let packages: Vec<(&str, Vec<&str>)> = entry["eager_packages"]
        .as_array()
        .expect("eager_packages array")
        .iter()
        .map(|package| {
            (
                package["name"].as_str().expect("name"),
                package["specifiers"]
                    .as_array()
                    .expect("specifiers")
                    .iter()
                    .map(|s| s.as_str().expect("specifier"))
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        packages,
        [
            ("clsx", vec!["clsx"]),
            ("lodash", vec!["lodash/debounce"]),
            ("react", vec!["react"]),
            ("zod", vec!["zod"])
        ],
        "a package re-export loads eagerly; a type-only re-export (type-fest) and a package behind import() (chart-lib) do not"
    );
    assert_eq!(entry["eager_package_count"], 4);
    assert!(
        entry["dominating_imports"]
            .as_array()
            .expect("dominating_imports array")
            .iter()
            .all(|import| import["target"] != "src/decl.d.ts"),
        "a value import of a declaration file loads nothing, so it is not counted"
    );

    let first = &entry["dominating_imports"][0];
    assert_eq!(first["importer"], "src/index.ts");
    assert_eq!(first["line"], 5);
    assert_eq!(first["target"], "src/heavy/view.ts");
    assert_eq!(first["exclusive_modules"], 3);
    assert_eq!(
        first["exclusive_bytes"],
        fixture_file_bytes(&[
            "src/heavy/view.ts",
            "src/heavy/chart-data.ts",
            "src/heavy/formatters.ts",
        ])
    );
}

#[test]
fn list_entry_weight_human_names_the_unit_and_the_heaviest_import() {
    let output = run_list(ENTRY_WEIGHT_FIXTURE, &["--entry-weight"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let text = format!("{}{}", output.stdout, output.stderr);
    assert!(text.contains("source bytes"), "{text}");
    assert!(text.contains("not bundle size"), "{text}");
    assert!(
        text.contains("src/index.ts:5 -> src/heavy/view.ts"),
        "{text}"
    );
}

fn run_entry_weight(args: &[&str]) -> CommandOutput {
    let mut all = vec!["--entry-weight", "--format", "json", "--quiet"];
    all.extend_from_slice(args);
    run_list(ENTRY_WEIGHT_FIXTURE, &all)
}

/// Save an entry weight baseline, then let `edit` change the saved JSON.
fn saved_entry_weight_baseline(
    dir: &std::path::Path,
    edit: impl FnOnce(&mut serde_json::Value),
) -> String {
    let path = dir.join("regression-baseline.json");
    let path_str = path.to_string_lossy().to_string();
    let save = run_entry_weight(&["--save-regression-baseline", &path_str]);
    assert_eq!(save.code, 0, "stderr: {}", save.stderr);
    let mut baseline: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("baseline written"))
            .expect("baseline is JSON");
    edit(&mut baseline);
    fs::write(&path, serde_json::to_string_pretty(&baseline).unwrap()).unwrap();
    path_str
}

fn shrink_saved_eager_bytes(baseline: &mut serde_json::Value, by: u64) {
    let entry = &mut baseline["entry_weight"]["entries"][0];
    let bytes = entry["eager_bytes"].as_u64().expect("eager_bytes saved");
    entry["eager_bytes"] = serde_json::json!(bytes - by);
}

#[test]
fn entry_weight_baseline_saves_each_entry_and_passes_on_an_unchanged_tree() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let baseline = saved_entry_weight_baseline(dir.path(), |saved| {
        let entry = &saved["entry_weight"]["entries"][0];
        assert_eq!(entry["path"], "src/index.ts");
        assert_eq!(entry["eager_modules"], 9);
        assert_eq!(
            entry["eager_packages"],
            serde_json::json!(["clsx", "lodash", "react", "zod"])
        );
    });

    let output = run_entry_weight(&["--regression-baseline", &baseline, "--fail-on-regression"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let regression = &parse_json(&output)["entry_weight"]["regression"];
    assert_eq!(regression["enforced"], true);
    assert_eq!(regression["exceeded"], false);
    assert_eq!(regression["entries"][0]["exceeded"], false);
}

#[test]
fn entry_weight_growth_is_report_only_unless_fail_on_regression_is_set() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let baseline = saved_entry_weight_baseline(dir.path(), |saved| {
        shrink_saved_eager_bytes(saved, 100);
        saved["entry_weight"]["entries"][0]["eager_packages"] =
            serde_json::json!(["clsx", "lodash", "zod"]);
    });

    let report_only = run_entry_weight(&["--regression-baseline", &baseline]);
    assert_eq!(report_only.code, 0, "stderr: {}", report_only.stderr);
    let regression = &parse_json(&report_only)["entry_weight"]["regression"];
    assert_eq!(regression["enforced"], false);
    assert_eq!(regression["exceeded"], true);
    let row = &regression["entries"][0];
    assert_eq!(
        row["current_eager_bytes"].as_u64().unwrap()
            - row["baseline_eager_bytes"].as_u64().unwrap(),
        100
    );
    assert_eq!(row["new_eager_packages"], serde_json::json!(["react"]));

    let gated = run_entry_weight(&["--regression-baseline", &baseline, "--fail-on-regression"]);
    assert_eq!(gated.code, 1, "stderr: {}", gated.stderr);

    let tolerated = run_entry_weight(&[
        "--regression-baseline",
        &baseline,
        "--fail-on-regression",
        "--tolerance",
        "100",
    ]);
    assert_eq!(
        tolerated.code, 0,
        "a byte tolerance of 100 allows 100 bytes of growth"
    );
    let percent = run_entry_weight(&[
        "--regression-baseline",
        &baseline,
        "--fail-on-regression",
        "--tolerance",
        "5%",
    ]);
    assert_eq!(percent.code, 0, "5% of the baseline bytes allows 100 bytes");
}

#[test]
fn entry_weight_baseline_save_keeps_the_saved_issue_counts() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("regression-baseline.json");
    fs::write(
        &path,
        r#"{"schema_version":2,"fallow_version":"0.0.0","timestamp":"t","check":{"total_issues":3,"unused_files":3}}"#,
    )
    .unwrap();
    let save = run_entry_weight(&["--save-regression-baseline", &path.to_string_lossy()]);
    assert_eq!(save.code, 0, "stderr: {}", save.stderr);
    let saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["check"]["total_issues"], 3);
    assert!(saved["entry_weight"]["entries"].is_array());
}

#[test]
fn entry_weight_gate_without_a_baseline_file_exits_two() {
    let output = run_entry_weight(&["--fail-on-regression"]);
    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
}
