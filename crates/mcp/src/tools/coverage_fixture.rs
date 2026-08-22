//! Shared git fixture for the `audit` and `check_health` Istanbul coverage
//! tests (#2368): a project whose second commit introduces a `branchy`
//! function with cyclomatic complexity 7, plus an Istanbul map recorded under
//! `/ci/workspace` so `health.coverageRoot` has to rebase it.
//!
//! With a covered map `branchy` scores CRAP 7 (below a `max_crap` of 10);
//! without any map the reachability estimate scores it 56.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Absolute prefix the Istanbul map records its files under.
pub(super) const COVERAGE_ROOT: &str = "/ci/workspace";

/// Project-relative location of the Istanbul map.
pub(super) const COVERAGE_FILE: &str = "artifacts/coverage-final.json";

/// A committed project with `branchy` introduced in `HEAD` and an Istanbul
/// map on disk. Drop the guard to delete the directory.
pub(super) struct CoverageFixture {
    dir: tempfile::TempDir,
}

impl CoverageFixture {
    /// Build the fixture. `covered` controls whether the map records
    /// `branchy` as called once or never.
    pub(super) fn new(covered: bool) -> Self {
        let dir = tempfile::tempdir().expect("fixture dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).expect("create src");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"mcp-coverage","type":"module","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::write(
            root.join("src/index.ts"),
            "import { used } from './utils';\nused();\n",
        )
        .expect("write entry");
        std::fs::write(root.join("src/utils.ts"), "export const used = () => 42;\n")
            .expect("write utils");
        git(root, &["init", "-b", "main"]);
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "initial"]);

        std::fs::write(
            root.join("src/index.ts"),
            "import { used } from './utils';\n\
             used();\n\
             function branchy(n: number): number {\n\
               if (n < 0) return -1;\n\
               if (n === 0) return 0;\n\
               if (n < 10) return 1;\n\
               if (n < 100) return 2;\n\
               if (n < 1000) return 3;\n\
               if (n < 10000) return 4;\n\
               return 5;\n\
             }\n\
             branchy(used());\n",
        )
        .expect("write branchy");
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "add branchy"]);

        let fixture = Self { dir };
        fixture.write_coverage(COVERAGE_FILE, covered);
        fixture
    }

    pub(super) fn root_string(&self) -> String {
        self.dir.path().display().to_string()
    }

    pub(super) fn coverage_path(&self) -> PathBuf {
        self.path(COVERAGE_FILE)
    }

    pub(super) fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    /// Write an Istanbul map for `branchy` at the project-relative `file`.
    pub(super) fn write_coverage(&self, file: &str, covered: bool) {
        self.write_coverage_for(file, "src/index.ts", covered);
    }

    /// Write a map that records `branchy` in a different file, so it matches
    /// nothing and CRAP falls back to the estimate.
    pub(super) fn write_stale_coverage(&self, file: &str) {
        self.write_coverage_for(file, "src/other.ts", true);
    }

    fn write_coverage_for(&self, file: &str, source_file: &str, covered: bool) {
        let path = self.path(file);
        std::fs::create_dir_all(path.parent().expect("coverage parent")).expect("create parent");
        let source = format!("{COVERAGE_ROOT}/{source_file}");
        let map = serde_json::json!({
            &source: {
                "path": source,
                "statementMap": {},
                "fnMap": {
                    "0": {
                        "name": "branchy",
                        "line": 3,
                        "decl": {
                            "start": { "line": 3, "column": 9 },
                            "end": { "line": 3, "column": 16 }
                        },
                        "loc": {
                            "start": { "line": 3, "column": 35 },
                            "end": { "line": 11, "column": 10 }
                        }
                    }
                },
                "branchMap": {},
                "s": {},
                "f": { "0": u8::from(covered) },
                "b": {}
            }
        });
        std::fs::write(path, serde_json::to_string(&map).expect("serialize map"))
            .expect("write coverage");
    }

    /// Write `.fallowrc.json` with the given `health` section body and return
    /// its path.
    pub(super) fn write_health_config(&self, health: &str) -> PathBuf {
        let path = self.dir.path().join(".fallowrc.json");
        std::fs::write(&path, format!(r#"{{"health":{health}}}"#)).expect("write config");
        path
    }

    /// `health` section pointing at the fixture map through the
    /// project-relative path and the recorded `/ci/workspace` root.
    pub(super) fn config_health_section() -> String {
        format!(r#"{{"maxCrap":10,"coverage":"{COVERAGE_FILE}","coverageRoot":"{COVERAGE_ROOT}"}}"#)
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("git command");
    assert!(status.success(), "git {args:?} failed");
}

/// The `branchy` complexity finding from a health or audit JSON payload.
pub(super) fn branchy_finding(findings: &serde_json::Value) -> Option<&serde_json::Value> {
    findings
        .as_array()?
        .iter()
        .find(|finding| finding["name"].as_str() == Some("branchy"))
}
