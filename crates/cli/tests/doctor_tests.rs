#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow_raw, run_fallow_raw_with_env};

fn cache_snapshot(
    path: &std::path::Path,
) -> Vec<(std::ffi::OsString, Vec<u8>, std::time::SystemTime)> {
    let mut files: Vec<_> = std::fs::read_dir(path)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name(),
                std::fs::read(entry.path()).unwrap(),
                entry.metadata().unwrap().modified().unwrap(),
            )
        })
        .collect();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

#[test]
fn doctor_honors_cache_env_precedence_without_mutating_the_cache() {
    let absolute_cache = tempfile::tempdir().unwrap();
    for cache_value in [
        "cache-env".to_string(),
        absolute_cache.path().to_string_lossy().into_owned(),
        String::new(),
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("package.json"),
            r#"{"name":"doctor-cache-env","main":"index.ts"}"#,
        )
        .unwrap();
        std::fs::write(root.path().join("index.ts"), "console.log('cache');\n").unwrap();
        std::fs::write(
            root.path().join("fallow.toml"),
            "[cache]\ndir = 'configured-cache'\n",
        )
        .unwrap();
        let root_text = root.path().to_string_lossy();
        let env = [("FALLOW_CACHE_DIR", cache_value.as_str())];
        let analysis = run_fallow_raw_with_env(
            &[
                "dead-code",
                "--root",
                &root_text,
                "--format",
                "json",
                "--quiet",
            ],
            &env,
        );
        assert!(matches!(analysis.code, 0 | 1), "{}", analysis.stderr);
        let cache_dir = root.path().join(if cache_value.is_empty() {
            "configured-cache"
        } else {
            &cache_value
        });
        assert!(cache_dir.join("cache.bin").exists());
        assert!(cache_dir.join("graph-cache.bin").exists());
        let before = cache_snapshot(&cache_dir);

        let output = run_fallow_raw_with_env(
            &[
                "doctor", "--root", &root_text, "--format", "json", "--quiet",
            ],
            &env,
        );
        assert_eq!(output.code, 0, "{}", output.stderr);
        let json = parse_json(&output);
        for (id, expected) in [("cache", "reusable"), ("graph-cache", "loads")] {
            let check = json["checks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|check| check["id"] == id)
                .unwrap();
            assert_eq!(check["status"], "pass");
            assert!(
                check["message"].as_str().unwrap().contains(expected),
                "{check}"
            );
        }
        assert_eq!(
            cache_snapshot(&cache_dir),
            before,
            "doctor must only read the selected cache"
        );
        assert!(!root.path().join(".fallow/cache.bin").exists());
        if !cache_value.is_empty() {
            assert!(!root.path().join("configured-cache").exists());
        }
    }
}

#[test]
fn help_only_advertises_supported_options() {
    for help_flag in ["-h", "--help"] {
        let output = run_fallow_raw(&["doctor", help_flag]);

        assert_eq!(output.code, 0, "doctor help failed: {}", output.stderr);
        assert!(output.stderr.is_empty());
        for supported in ["--root", "--config", "--format", "--pretty", "--quiet"] {
            assert!(
                output.stdout.contains(supported),
                "doctor {help_flag} omitted supported option {supported}"
            );
        }
        for unsupported in [
            "--output-file",
            "--allow-remote-extends",
            "--changed-since",
            "--workspace",
            "--ci",
            "--type-aware",
            "sarif",
            "compact",
        ] {
            assert!(
                !output.stdout.contains(unsupported),
                "doctor {help_flag} advertised unsupported option {unsupported}"
            );
        }
    }
}

#[test]
fn output_file_is_rejected_without_creating_or_modifying_a_target() {
    let root = tempfile::tempdir().expect("temp root");
    let output_path = root.path().join("doctor.json");
    let root_text = root.path().to_string_lossy();
    let output_text = output_path.to_string_lossy();
    let output = run_fallow_raw(&[
        "doctor",
        "--root",
        root_text.as_ref(),
        "--format",
        "json",
        "--output-file",
        output_text.as_ref(),
        "--quiet",
    ]);

    assert_eq!(output.code, 2);
    assert!(!output_path.exists());
    assert!(output.stderr.is_empty());
    let json = parse_json(&output);
    assert_eq!(json["error"], true);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("writes its report to stdout"))
    );

    let original = b"important user content\n";
    std::fs::write(&output_path, original).expect("write existing file");
    let output = run_fallow_raw(&[
        "doctor",
        "--root",
        root_text.as_ref(),
        "--output-file",
        output_text.as_ref(),
        "--quiet",
    ]);
    assert_eq!(output.code, 2);
    assert_eq!(std::fs::read(&output_path).expect("read output"), original);
    assert!(output.stderr.contains("writes its report to stdout"));
}

#[test]
fn zero_config_json_is_stable_and_path_free() {
    let root = tempfile::tempdir().expect("temp root");
    let root_text = root.path().to_string_lossy();
    let output = run_fallow_raw(&[
        "doctor",
        "--root",
        root_text.as_ref(),
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 0, "doctor failed: {}", output.stderr);
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.contains(root_text.as_ref()));
    let json = parse_json(&output);
    assert_eq!(json["kind"], "doctor");
    assert_eq!(json["schema_version"], 2);
    assert_eq!(json["root"], ".");
    // A bare temp root has no node_modules, which is exactly the state the
    // dependencies check exists to report, so the aggregate is advisory.
    assert_eq!(json["status"], "warn");
    assert_eq!(
        json["checks"]
            .as_array()
            .expect("checks array")
            .iter()
            .map(|check| check["id"].as_str().expect("check id"))
            .collect::<Vec<_>>(),
        [
            "root",
            "config",
            "workspaces",
            "plugins",
            "type-aware",
            "dependencies",
            "cache",
            "graph-cache"
        ]
    );
}

#[test]
fn invalid_config_returns_complete_failed_json_report() {
    let root = tempfile::tempdir().expect("temp root");
    std::fs::write(root.path().join(".fallowrc.json"), "{").expect("write invalid config");
    let root_text = root.path().to_string_lossy();
    let output = run_fallow_raw(&[
        "doctor",
        "--root",
        root_text.as_ref(),
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 2);
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.contains(root_text.as_ref()));
    let json = parse_json(&output);
    assert_eq!(json["kind"], "doctor");
    assert_eq!(json["status"], "fail");
    assert_eq!(json["checks"].as_array().map(Vec::len), Some(8));
    assert_eq!(json["checks"][1]["id"], "config");
    assert_eq!(json["checks"][1]["status"], "fail");
}

#[test]
fn human_report_is_readable_and_path_free() {
    let root = tempfile::tempdir().expect("temp root");
    let root_text = root.path().to_string_lossy();
    let output = run_fallow_raw(&["doctor", "--root", root_text.as_ref(), "--quiet"]);

    assert_eq!(output.code, 0, "doctor failed: {}", output.stderr);
    assert!(output.stdout.starts_with("Fallow doctor (.)\n"));
    assert!(output.stdout.contains("[OK] root:"));
    assert!(output.stdout.contains("Status: ready"));
    assert!(!output.stdout.contains(root_text.as_ref()));
}

#[test]
fn invalid_root_uses_shared_failure_prefix_and_actionable_private_message() {
    let root = tempfile::tempdir().expect("temp root");
    let missing = root.path().join("missing");
    let missing_text = missing.to_string_lossy();
    let output = run_fallow_raw(&["doctor", "--root", missing_text.as_ref(), "--quiet"]);

    assert_eq!(output.code, 2);
    assert!(output.stderr.is_empty());
    assert!(output.stdout.contains("[X] root:"));
    assert!(
        output
            .stdout
            .contains("Set --root to an existing, readable directory.")
    );
    assert!(!output.stdout.contains(missing_text.as_ref()));
}

#[test]
fn analysis_flags_are_rejected_instead_of_ignored() {
    let output = run_fallow_raw(&["doctor", "--changed-since", "main", "--format", "json"]);

    assert_eq!(output.code, 2);
    let json = parse_json(&output);
    assert_eq!(json["error"], true);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("--changed-since"))
    );
}

#[test]
fn type_aware_environment_requirement_controls_readiness() {
    let root = tempfile::tempdir().expect("temp root");
    let root_text = root.path().to_string_lossy();
    let output = run_fallow_raw_with_env(
        &[
            "doctor",
            "--root",
            root_text.as_ref(),
            "--format",
            "json",
            "--quiet",
        ],
        &[
            ("FALLOW_TYPE_AWARE", "true"),
            ("FALLOW_TYPE_AWARE_REQUIRE", "complete"),
            (
                "FALLOW_TYPE_AWARE_BIN",
                "/definitely/missing/fallow-type-aware",
            ),
        ],
    );

    assert_eq!(output.code, 2);
    let json = parse_json(&output);
    assert_eq!(json["status"], "fail");
    assert_eq!(json["checks"][4]["id"], "type-aware");
    assert_eq!(json["checks"][4]["status"], "fail");
    assert_eq!(json["checks"][4]["required"], true);
    assert_eq!(json["checks"][4]["remediation"]["cwd"], ".");
    assert_eq!(json["checks"][4]["remediation"]["mutating"], true);
}

#[test]
fn doctor_rejects_fail_on_stale_baseline() {
    let output = run_fallow_raw(&["doctor", "--fail-on-stale-baseline"]);
    assert_eq!(
        output.code, 2,
        "doctor must reject analysis flags: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("--fail-on-stale-baseline"),
        "the refusal must name the flag: {}",
        output.stderr
    );
}
