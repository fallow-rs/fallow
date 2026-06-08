#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! End-to-end integration tests for `fallow health --runtime-coverage`.
//!
//! Exercises the full CLI → sidecar pipeline with a signed stub sidecar:
//! discovery via `FALLOW_COV_BIN`, Ed25519 signature verification, Request
//! marshalling over stdin, Response parsing, protocol-version check, and the
//! 3 / 4 / 5 / 6 exit-code matrix. Pairs with the source-level
//! network-prohibition assertion in
//! `crates/cli/src/health/coverage.rs::tests::runtime_coverage_module_has_no_network_code`
//! to cover the Phase 2 step 4 roadmap gate:
//!
//! > integration test asserting zero network calls during analysis.
//!
//! Gated behind the `test-sidecar-key` cargo feature: the feature swaps both
//! the sidecar binary-signing pubkey and the license JWT pubkey for
//! deterministic test keypairs, and activates the `stub_sidecar` bin target.
//! A `compile_error!` in `coverage.rs` blocks the feature from release builds.
//!
//! Run: `cargo test -p fallow-cli --features test-sidecar-key runtime_coverage`.

#[path = "common/mod.rs"]
mod common;

#[cfg(feature = "test-sidecar-key")]
#[path = "common/sign.rs"]
mod sign;

#[cfg(feature = "test-sidecar-key")]
mod gated {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use tempfile::TempDir;

    use super::common::{fallow_bin, fixture_path, parse_json};
    use super::sign;

    struct Harness {
        tmp: TempDir,
        home: PathBuf,
        coverage_file: PathBuf,
        stub_bin: PathBuf,
    }

    impl Harness {
        fn new() -> Self {
            let tmp = tempfile::tempdir().expect("create temp dir");
            let root = tmp.path();

            let home = root.join("home");
            fs::create_dir_all(&home).expect("create fake home");

            let coverage_file = root.join("coverage-final-v8.json");
            fs::write(&coverage_file, br#"{"result":[]}"#).expect("write coverage input");

            let stub_bin = copy_and_sign_stub(root);

            Self {
                tmp,
                home,
                coverage_file,
                stub_bin,
            }
        }

        fn fallow(&self) -> Command {
            let mut cmd = Command::new(fallow_bin());
            cmd.env("NO_COLOR", "1");
            cmd.env("RUST_LOG", "");
            cmd.env_remove("FALLOW_LICENSE");
            cmd.env_remove("FALLOW_LICENSE_PATH");
            cmd.env_remove("FALLOW_COV_BINARY_PATH");
            cmd.env_remove("FALLOW_COVERAGE");
            cmd.env_remove("FALLOW_BIN");
            cmd.env_remove("FALLOW_FORMAT");
            cmd.env_remove("FALLOW_QUIET");
            cmd.env("HOME", &self.home);
            cmd.env("USERPROFILE", &self.home);
            cmd.env("FALLOW_COV_BIN", &self.stub_bin);
            cmd
        }

        fn health_args(&self) -> Vec<String> {
            self.health_args_with_format("json")
        }

        fn health_args_with_format(&self, format: &str) -> Vec<String> {
            Self::health_args_for_path_with_format(&self.coverage_file, format)
        }

        fn health_args_for_path_with_format(coverage_path: &Path, format: &str) -> Vec<String> {
            let fixture = fixture_path("coverage-gaps");
            vec![
                "health".to_owned(),
                "--root".to_owned(),
                fixture.to_string_lossy().into_owned(),
                "--runtime-coverage".to_owned(),
                coverage_path.to_string_lossy().into_owned(),
                "--format".to_owned(),
                format.to_owned(),
                "--quiet".to_owned(),
            ]
        }

        fn security_args(&self) -> Vec<String> {
            let fixture = fixture_path("security-dangerous-html");
            vec![
                "security".to_owned(),
                "--root".to_owned(),
                fixture.to_string_lossy().into_owned(),
                "--runtime-coverage".to_owned(),
                self.coverage_file.to_string_lossy().into_owned(),
                "--format".to_owned(),
                "json".to_owned(),
                "--quiet".to_owned(),
            ]
        }

        fn multi_capture_dir(&self) -> PathBuf {
            let dir = self.tmp.path().join("multi-capture");
            fs::create_dir_all(&dir).expect("create multi-capture dir");
            fs::write(dir.join("capture-a.json"), br#"{"result":[]}"#)
                .expect("write first capture");
            fs::write(dir.join("capture-b.json"), br#"{"result":[]}"#)
                .expect("write second capture");
            dir
        }
    }

    fn copy_and_sign_stub(root: &Path) -> PathBuf {
        let source = PathBuf::from(env!("CARGO_BIN_EXE_stub_sidecar"));
        let target_name = if cfg!(windows) {
            "fallow-cov.exe"
        } else {
            "fallow-cov"
        };
        let target = root.join(target_name);
        fs::copy(&source, &target).expect("copy stub sidecar");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&target).expect("stat stub").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&target, perms).expect("chmod stub");
        }

        sign::sign_sidecar_binary(&target);
        target
    }

    fn run_with(mut cmd: Command) -> (String, String, i32) {
        let output = cmd.output().expect("run fallow binary");
        (
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
            output.status.code().unwrap_or(-1),
        )
    }

    #[test]
    fn license_missing_local_single_capture_succeeds() {
        let harness = Harness::new();
        let mut cmd = harness.fallow();
        cmd.env("FALLOW_STUB_MODE", "ok");
        for arg in harness.health_args() {
            cmd.arg(arg);
        }
        let (stdout, stderr, code) = run_with(cmd);
        assert_eq!(
            code, 0,
            "missing license must still allow single-capture local analysis; stdout={stdout}, stderr={stderr}"
        );
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
            panic!(
                "expected JSON output; err={err}; stdout head={}",
                &stdout.chars().take(400).collect::<String>()
            )
        });
        assert_eq!(
            json.pointer("/runtime_coverage/schema_version"),
            Some(&serde_json::Value::String("1".to_owned()))
        );
    }

    #[test]
    fn license_missing_paid_shape_exits_3() {
        let harness = Harness::new();
        let mut cmd = harness.fallow();
        cmd.env("FALLOW_STUB_MODE", "enforce-license-gate");
        let coverage_dir = harness.multi_capture_dir();
        for arg in Harness::health_args_for_path_with_format(&coverage_dir, "human") {
            cmd.arg(arg);
        }
        let (stdout, stderr, code) = run_with(cmd);
        assert_eq!(
            code, 3,
            "missing license for paid shape must exit 3; stdout={stdout}, stderr={stderr}"
        );
        assert!(
            stderr.contains("continuous") || stderr.contains("license"),
            "paid-shape error should mention continuous analysis or license; stderr={stderr}"
        );
    }

    #[test]
    fn license_expired_hard_fail_exits_3() {
        let harness = Harness::new();
        let mut cmd = harness.fallow();
        cmd.env("FALLOW_LICENSE", sign::mint_expired_runtime_coverage_jwt());
        for arg in harness.health_args() {
            cmd.arg(arg);
        }
        let (stdout, stderr, code) = run_with(cmd);
        assert_eq!(
            code, 3,
            "expired hard-fail license must exit 3; stdout={stdout}, stderr={stderr}"
        );
    }

    #[test]
    fn happy_path_exits_0_and_renders_runtime_coverage() {
        let harness = Harness::new();
        let mut cmd = harness.fallow();
        cmd.env("FALLOW_LICENSE", sign::mint_runtime_coverage_jwt());
        cmd.env("FALLOW_STUB_MODE", "ok");
        for arg in harness.health_args() {
            cmd.arg(arg);
        }
        let (stdout, stderr, code) = run_with(cmd);
        assert_eq!(
            code,
            0,
            "happy path must exit 0; stderr={stderr}; stdout head={}",
            &stdout.chars().take(400).collect::<String>()
        );
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
            panic!(
                "expected JSON output; err={err}; stdout head={}",
                &stdout.chars().take(400).collect::<String>()
            )
        });
        assert!(
            json.get("runtime_coverage").is_some(),
            "runtime_coverage key missing from JSON output; keys={:?}",
            json.as_object().map(|o| o.keys().collect::<Vec<_>>())
        );
        assert_eq!(
            json.pointer("/runtime_coverage/schema_version"),
            Some(&serde_json::Value::String("1".to_owned())),
            "runtime_coverage schema_version must be stable for agent consumers"
        );
    }

    #[test]
    fn security_runtime_coverage_marks_hot_sink_candidate() {
        let harness = Harness::new();
        let mut cmd = harness.fallow();
        cmd.env("FALLOW_LICENSE", sign::mint_runtime_coverage_jwt());
        cmd.env("FALLOW_STUB_MODE", "security-hot");
        for arg in harness.security_args() {
            cmd.arg(arg);
        }
        let (stdout, stderr, code) = run_with(cmd);
        assert_eq!(
            code,
            0,
            "security runtime coverage must exit 0; stderr={stderr}; stdout head={}",
            &stdout.chars().take(400).collect::<String>()
        );
        let json: serde_json::Value =
            serde_json::from_str(&stdout).expect("security output should be JSON");
        let first = json
            .pointer("/security_findings/0")
            .expect("first security finding");

        assert_eq!(
            first.pointer("/path"),
            Some(&serde_json::Value::String("src/sink.ts".to_owned()))
        );
        assert_eq!(
            first.pointer("/runtime/state"),
            Some(&serde_json::Value::String("runtime-hot".to_owned()))
        );
        assert_eq!(
            first.pointer("/runtime/invocations"),
            Some(&serde_json::Value::Number(250.into()))
        );
    }

    #[test]
    fn sidecar_missing_exits_4() {
        let harness = Harness::new();
        let mut cmd = harness.fallow();
        cmd.env("FALLOW_LICENSE", sign::mint_runtime_coverage_jwt());
        cmd.env("FALLOW_COV_BIN", harness.home.join("does-not-exist"));
        for arg in harness.health_args() {
            cmd.arg(arg);
        }
        let (stdout, stderr, code) = run_with(cmd);
        assert_eq!(
            code, 4,
            "missing sidecar must exit 4; stdout={stdout}; stderr={stderr}"
        );
        let combined = format!("{stdout}{stderr}");
        assert!(
            combined.contains("FALLOW_COV_BIN"),
            "error message should mention FALLOW_COV_BIN; got:\n{combined}"
        );
    }

    #[test]
    fn sidecar_exits_4_marshalled_to_cli_exit_4() {
        exit_code_case("exit-4", 4);
    }

    #[test]
    fn sidecar_exits_5_marshalled_to_cli_exit_5() {
        exit_code_case("exit-5", 5);
    }

    #[test]
    fn sidecar_exits_6_marshalled_to_cli_exit_6() {
        exit_code_case("exit-6", 6);
    }

    #[test]
    fn sidecar_protocol_mismatch_exits_4() {
        let harness = Harness::new();
        let mut cmd = harness.fallow();
        cmd.env("FALLOW_LICENSE", sign::mint_runtime_coverage_jwt());
        cmd.env("FALLOW_STUB_MODE", "protocol-mismatch");
        for arg in harness.health_args() {
            cmd.arg(arg);
        }
        let (stdout, stderr, code) = run_with(cmd);
        assert_eq!(
            code, 4,
            "protocol-version mismatch must exit 4; stdout={stdout}; stderr={stderr}"
        );
    }

    #[test]
    fn malformed_sidecar_stdout_exits_4() {
        let harness = Harness::new();
        let mut cmd = harness.fallow();
        cmd.env("FALLOW_LICENSE", sign::mint_runtime_coverage_jwt());
        cmd.env("FALLOW_STUB_MODE", "malformed-stdout");
        for arg in harness.health_args() {
            cmd.arg(arg);
        }
        let (stdout, stderr, code) = run_with(cmd);
        assert_eq!(
            code, 4,
            "non-JSON sidecar stdout must exit 4; stdout={stdout}; stderr={stderr}"
        );
    }

    #[test]
    fn bad_sidecar_signature_exits_4() {
        let harness = Harness::new();
        let mut sig_os = harness.stub_bin.as_os_str().to_os_string();
        sig_os.push(".sig");
        let sig_path = PathBuf::from(sig_os);
        fs::write(&sig_path, [0u8; 64]).expect("overwrite signature");

        let mut cmd = harness.fallow();
        cmd.env("FALLOW_LICENSE", sign::mint_runtime_coverage_jwt());
        cmd.env("FALLOW_STUB_MODE", "ok");
        for arg in harness.health_args() {
            cmd.arg(arg);
        }
        let (stdout, stderr, code) = run_with(cmd);
        assert_eq!(
            code, 4,
            "bad signature must exit 4; stdout={stdout}; stderr={stderr}"
        );
    }

    #[test]
    fn happy_path_does_not_set_watermark() {
        let harness = Harness::new();
        let mut cmd = harness.fallow();
        cmd.env("FALLOW_LICENSE", sign::mint_runtime_coverage_jwt());
        cmd.env("FALLOW_STUB_MODE", "ok");
        for arg in harness.health_args() {
            cmd.arg(arg);
        }
        let (stdout, _stderr, code) = run_with(cmd);
        assert_eq!(code, 0);
        let json = parse_json(&super::common::CommandOutput {
            stdout,
            stderr: String::new(),
            code,
        });
        let coverage = json
            .get("runtime_coverage")
            .expect("runtime_coverage present");
        let watermark = coverage.get("watermark");
        assert!(
            watermark.is_none_or(serde_json::Value::is_null),
            "fresh license must not emit a watermark; got {watermark:?}"
        );
    }

    #[test]
    fn capture_quality_short_renders_warning_and_upgrade_prompt_in_human_output() {
        let harness = Harness::new();
        let mut cmd = harness.fallow();
        cmd.env("FALLOW_LICENSE", sign::mint_runtime_coverage_jwt());
        cmd.env("FALLOW_STUB_MODE", "capture-quality-short");
        for arg in harness.health_args_with_format("human") {
            cmd.arg(arg);
        }
        let (stdout, stderr, code) = run_with(cmd);
        let combined = format!("{stdout}{stderr}");
        assert_eq!(
            code, 0,
            "short-capture happy path must exit 0; combined={combined}"
        );
        assert!(
            combined.contains("note: short capture (12 min from 1 instance)"),
            "short-capture warning banner missing; combined={combined}"
        );
        assert!(
            combined.contains("lazy-parsed scripts may not appear"),
            "lazy-parse guidance missing; combined={combined}"
        );
        assert!(
            combined.contains("captured 12 min from 1 instance."),
            "quantified upgrade prompt header missing; combined={combined}"
        );
        assert!(
            combined.contains("continuous monitoring over 30 days"),
            "upgrade prompt body missing; combined={combined}"
        );
        assert!(
            combined.contains("fallow license activate --trial"),
            "trial CTA command missing; combined={combined}"
        );
    }

    #[test]
    fn capture_quality_long_shows_neither_warning_nor_upgrade_prompt() {
        let harness = Harness::new();
        let mut cmd = harness.fallow();
        cmd.env("FALLOW_LICENSE", sign::mint_runtime_coverage_jwt());
        cmd.env("FALLOW_STUB_MODE", "capture-quality-long");
        for arg in harness.health_args_with_format("human") {
            cmd.arg(arg);
        }
        let (stdout, stderr, code) = run_with(cmd);
        let combined = format!("{stdout}{stderr}");
        assert_eq!(
            code, 0,
            "long-capture happy path must exit 0; combined={combined}"
        );
        assert!(
            !combined.contains("short capture"),
            "long capture must not emit the short-capture warning; combined={combined}"
        );
        assert!(
            !combined.contains("start a trial"),
            "long capture must not emit the trial CTA; combined={combined}"
        );
    }

    #[test]
    fn capture_quality_short_does_not_emit_upgrade_prompt_in_json() {
        let harness = Harness::new();
        let mut cmd = harness.fallow();
        cmd.env("FALLOW_LICENSE", sign::mint_runtime_coverage_jwt());
        cmd.env("FALLOW_STUB_MODE", "capture-quality-short");
        for arg in harness.health_args_with_format("json") {
            cmd.arg(arg);
        }
        let (stdout, stderr, code) = run_with(cmd);
        assert_eq!(
            code, 0,
            "json short-capture run must exit 0; stderr={stderr}"
        );
        assert!(
            !stdout.contains("start a trial"),
            "JSON format must never include the trial CTA free text; stdout={stdout}"
        );
        let json: serde_json::Value =
            serde_json::from_str(&stdout).expect("json output must parse");
        let quality = json
            .pointer("/runtime_coverage/summary/capture_quality")
            .expect("capture_quality absent from json summary");
        assert_eq!(
            quality.get("lazy_parse_warning"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(quality.get("window_seconds"), Some(&serde_json::json!(720)));
        assert_eq!(
            quality.get("instances_observed"),
            Some(&serde_json::json!(1))
        );
    }

    fn exit_code_case(mode: &str, expected: i32) {
        let harness = Harness::new();
        let mut cmd = harness.fallow();
        cmd.env("FALLOW_LICENSE", sign::mint_runtime_coverage_jwt());
        cmd.env("FALLOW_STUB_MODE", mode);
        for arg in harness.health_args() {
            cmd.arg(arg);
        }
        let (stdout, stderr, code) = run_with(cmd);
        assert_eq!(
            code, expected,
            "sidecar mode {mode} must map to CLI exit {expected}; stdout={stdout}; stderr={stderr}"
        );
    }
}
