#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! End-to-end integration test for the runtime-weighted focus map on
//! `fallow review` (the alias of `fallow audit --brief`).
//!
//! Drives the full CLI -> sidecar pipeline with a signed stub sidecar that
//! reports a hot path (`src/sink.ts::render`, 250 invocations), against a temp
//! git repo whose `src/sink.ts` is a reachable module changed since the base.
//! It asserts the brief's focus map weights that hot changed file with a
//! `runtime` score component and lifts it into `review-here`. A negative control
//! runs the same repo WITHOUT `--runtime-coverage` and asserts the focus map
//! carries no `runtime` component and no `skip` label (the no-runtime surface).
//!
//! This closes the seam the unit tests cannot reach on their own: that
//! `--runtime-coverage` actually populates `result.health.report.runtime_coverage`
//! and that `build_runtime_focus` joins it onto the focus map end-to-end.
//!
//! Gated behind the `test-sidecar-key` cargo feature, which swaps in the
//! deterministic test sidecar-signing and license keypairs and builds the
//! `stub_sidecar` bin (a `compile_error!` blocks the feature from release
//! builds). Run:
//!   cargo test -p fallow-cli --features test-sidecar-key runtime_focus

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

    use super::common::{CommandOutput, fallow_bin};
    use super::sign;

    /// Run git in `dir` with hermetic config + a fixed test identity.
    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Copy the test stub sidecar to `<root>/fallow-cov`, make it executable, and
    /// Ed25519-sign it so the CLI's signature check accepts it.
    fn copy_and_sign_stub(root: &Path) -> PathBuf {
        let source = PathBuf::from(env!("CARGO_BIN_EXE_stub_sidecar"));
        let target = root.join(if cfg!(windows) {
            "fallow-cov.exe"
        } else {
            "fallow-cov"
        });
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

    /// Build a temp git repo where `src/sink.ts` is a reachable graph module
    /// (imported + called by `src/index.ts`) AND is changed vs `HEAD~1`, so it is
    /// a unit on the brief's focus map. The stub reports `src/sink.ts::render` as
    /// the hot path, so this is the file the runtime layer weights.
    fn make_repo() -> TempDir {
        let tmp = TempDir::new().expect("temp dir");
        let dir = tmp.path();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("package.json"),
            r#"{"name":"rt-focus","main":"src/index.ts"}"#,
        )
        .unwrap();
        fs::write(
            dir.join("src/index.ts"),
            "import { render } from './sink';\nrender();\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/sink.ts"),
            "export function render(): number {\n  return 1;\n}\n",
        )
        .unwrap();
        git(dir, &["init", "-b", "main"]);
        git(dir, &["add", "."]);
        git(
            dir,
            &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
        );
        // Second commit edits src/sink.ts so it lands in the diff vs HEAD~1.
        fs::write(
            dir.join("src/sink.ts"),
            "export function render(): number {\n  return 2;\n}\n",
        )
        .unwrap();
        git(dir, &["add", "."]);
        git(
            dir,
            &["-c", "commit.gpgsign=false", "commit", "-m", "edit render"],
        );
        tmp
    }

    /// Run `fallow review --format json` against `repo`. With `runtime`, attaches
    /// the signed stub + a runtime-coverage license + the `security-hot` stub mode
    /// so the sidecar reports a hot path. `home` isolates license/cache state.
    fn run_review(repo: &Path, home: &Path, stub: &Path, runtime: bool) -> CommandOutput {
        let mut cmd = Command::new(fallow_bin());
        cmd.current_dir(repo);
        cmd.env("NO_COLOR", "1").env("RUST_LOG", "");
        cmd.env_remove("FALLOW_LICENSE");
        cmd.env_remove("FALLOW_LICENSE_PATH");
        cmd.env_remove("FALLOW_COV_BINARY_PATH");
        cmd.env_remove("FALLOW_COVERAGE");
        cmd.env_remove("FALLOW_BIN");
        cmd.env_remove("FALLOW_FORMAT");
        cmd.env_remove("FALLOW_QUIET");
        cmd.env("HOME", home).env("USERPROFILE", home);
        cmd.args([
            "review",
            "--root",
            &repo.to_string_lossy(),
            "--base",
            "HEAD~1",
            "--format",
            "json",
            "--quiet",
        ]);
        if runtime {
            // Written OUTSIDE the repo so it never enters the diff; the stub
            // ignores the path's contents, so an empty V8 shape is sufficient.
            let coverage = home.join("coverage-final-v8.json");
            fs::write(&coverage, br#"{"result":[]}"#).expect("write coverage input");
            cmd.env("FALLOW_COV_BIN", stub);
            cmd.env("FALLOW_LICENSE", sign::mint_runtime_coverage_jwt());
            cmd.env("FALLOW_STUB_MODE", "security-hot");
            cmd.args(["--runtime-coverage", &coverage.to_string_lossy()]);
        }
        let output = cmd.output().expect("run fallow binary");
        CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            code: output.status.code().unwrap_or(-1),
        }
    }

    /// Every focus unit (`review_here` ++ `deprioritized`).
    fn focus_units(json: &serde_json::Value) -> Vec<serde_json::Value> {
        let mut units = Vec::new();
        for key in ["review_here", "deprioritized"] {
            if let Some(arr) = json["focus"][key].as_array() {
                units.extend(arr.iter().cloned());
            }
        }
        units
    }

    fn parse(out: &CommandOutput) -> serde_json::Value {
        serde_json::from_str(&out.stdout).unwrap_or_else(|err| {
            panic!(
                "failed to parse brief JSON: {err}\nstdout:\n{}\nstderr:\n{}",
                out.stdout, out.stderr
            )
        })
    }

    #[test]
    fn review_runtime_coverage_weights_the_hot_changed_file() {
        let tmp = TempDir::new().expect("temp dir");
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let stub = copy_and_sign_stub(tmp.path());
        let repo = make_repo();

        let out = run_review(repo.path(), &home, &stub, true);
        assert_eq!(
            out.code, 0,
            "review always exits 0; stderr:\n{}",
            out.stderr
        );

        let json = parse(&out);
        let units = focus_units(&json);
        let sink = units
            .iter()
            .find(|unit| unit["file"] == "src/sink.ts")
            .unwrap_or_else(|| panic!("src/sink.ts missing from focus map: {}", json["focus"]));

        // The hot path adds a runtime weight, lifting the file into review-here.
        let runtime = sink["score"]["runtime"]
            .as_u64()
            .expect("runtime component present with --runtime-coverage");
        assert!(
            runtime > 0,
            "hot file carries a runtime weight, got {runtime}"
        );
        let total = sink["score"]["total"].as_u64().expect("total present");
        assert!(
            total >= runtime,
            "total ({total}) includes the runtime weight"
        );
        assert_eq!(
            sink["label"], "review-here",
            "hot file lands in review-here"
        );
        assert!(
            sink["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("hot path (250 invocations)"),
            "reason names the hot path, got {}",
            sink["reason"]
        );
    }

    #[test]
    fn review_without_runtime_coverage_has_no_runtime_component_or_skip() {
        let tmp = TempDir::new().expect("temp dir");
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let stub = copy_and_sign_stub(tmp.path());
        let repo = make_repo();

        let out = run_review(repo.path(), &home, &stub, false);
        assert_eq!(
            out.code, 0,
            "review always exits 0; stderr:\n{}",
            out.stderr
        );

        // Free mode (no runtime input): no skip label and no runtime component.
        assert!(
            !out.stdout.contains("\"skip\""),
            "free mode must never emit a skip label"
        );
        let json = parse(&out);
        let units = focus_units(&json);
        assert!(
            units.iter().any(|unit| unit["file"] == "src/sink.ts"),
            "src/sink.ts is still a focus unit in free mode"
        );
        for unit in &units {
            assert!(
                unit["score"].get("runtime").is_none(),
                "runtime component is omitted in free mode, unit={unit}"
            );
            assert_ne!(unit["label"], "skip", "no skip label in free mode");
        }
    }
}
