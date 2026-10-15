//! The test runner of a mutation run: the `mutation.runner` config value, or
//! a detection from the project files when the value is `auto`.

use std::fs;
use std::path::Path;

use fallow_config::MutationRunner;

use super::discovery::RuntimeKind;

/// A test runner that the companion has an adapter for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    Vitest,
    NodeTest,
    BunTest,
}

impl Runner {
    /// The runner name in the protocol manifest and in the report.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Vitest => "vitest",
            Self::NodeTest => "node-test",
            Self::BunTest => "bun-test",
        }
    }

    /// The runtimes that can start the companion for this runner, in the
    /// order of the `PATH` search. `node:test` needs Node, and `bun test`
    /// needs Bun. Vitest runs on both.
    pub const fn runtimes(self) -> &'static [RuntimeKind] {
        match self {
            Self::Vitest => &[RuntimeKind::Node, RuntimeKind::Bun],
            Self::NodeTest => &[RuntimeKind::Node],
            Self::BunTest => &[RuntimeKind::Bun],
        }
    }

    /// True when the runner loads a Vue, Svelte or Astro component file.
    /// Vitest loads it through the framework plugin of the project config.
    /// The `node:test` and `bun test` adapters serve the instrumented file
    /// through a module hook, and neither runtime has a loader for a
    /// component file.
    pub const fn loads_component_files(self) -> bool {
        matches!(self, Self::Vitest)
    }

    /// The step that installs the runtime of this runner.
    pub const fn runtime_help(self) -> &'static str {
        match self {
            Self::Vitest | Self::NodeTest => "install Node.js, or pass the runtime path",
            Self::BunTest => "install Bun, or pass the runtime path",
        }
    }
}

/// The most test files that the detection reads.
const MAX_SCANNED_TEST_FILES: usize = 200;

/// The Bun files at the project root that mark a Bun project.
const BUN_ROOT_FILES: &[&str] = &["bunfig.toml", "bun.lock", "bun.lockb"];

/// Selects the runner. An explicit config value wins. `auto` reads the
/// project at `root`, in this order:
///
/// 1. `vitest` in the root `package.json` dependencies gives Vitest.
/// 2. The import of `vitest`, `bun:test` or `node:test` that the most test
///    files have. A tie goes to Vitest, then `bun test`.
/// 3. A Bun file at the root (`bunfig.toml`, `bun.lock`, `bun.lockb`) gives
///    `bun test`.
/// 4. A `node --test` or `borp` command in a `package.json` script gives
///    `node:test`.
/// 5. Otherwise Vitest.
pub fn select_runner(configured: MutationRunner, root: &Path, test_files: &[String]) -> Runner {
    match configured {
        MutationRunner::Vitest => Runner::Vitest,
        MutationRunner::NodeTest => Runner::NodeTest,
        MutationRunner::BunTest => Runner::BunTest,
        MutationRunner::Auto => detect_runner(root, test_files),
    }
}

fn detect_runner(root: &Path, test_files: &[String]) -> Runner {
    let manifest = read_manifest(root);
    if manifest.as_ref().is_some_and(has_vitest_dependency) {
        return Runner::Vitest;
    }
    if let Some(runner) = runner_by_imports(root, test_files) {
        return runner;
    }
    if BUN_ROOT_FILES.iter().any(|file| root.join(file).is_file()) {
        return Runner::BunTest;
    }
    if manifest
        .as_ref()
        .is_some_and(|manifest| has_node_test_script(manifest) || has_borp_script(manifest))
    {
        return Runner::NodeTest;
    }
    Runner::Vitest
}

fn read_manifest(root: &Path) -> Option<serde_json::Value> {
    let text = fs::read_to_string(root.join("package.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn has_vitest_dependency(manifest: &serde_json::Value) -> bool {
    ["dependencies", "devDependencies", "optionalDependencies"]
        .iter()
        .any(|field| {
            manifest
                .get(field)
                .and_then(|deps| deps.get("vitest"))
                .is_some()
        })
}

fn has_node_test_script(manifest: &serde_json::Value) -> bool {
    manifest
        .get("scripts")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|scripts| {
            scripts
                .values()
                .filter_map(serde_json::Value::as_str)
                .any(|script| script.contains("node --test"))
        })
}

/// The name of the `borp` binary, a test runner that runs `node:test`.
const BORP_BINARY: &str = "borp";

/// True when a `package.json` script runs `borp`, also through a package
/// manager (`npx borp`) or a binary path (`./node_modules/.bin/borp`).
fn has_borp_script(manifest: &serde_json::Value) -> bool {
    script_texts(manifest).any(|script| {
        script
            .split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')'))
            .any(|word| word.rsplit('/').next() == Some(BORP_BINARY))
    })
}

fn script_texts(manifest: &serde_json::Value) -> impl Iterator<Item = &str> {
    manifest
        .get("scripts")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flat_map(|scripts| scripts.values().filter_map(serde_json::Value::as_str))
}

/// The runner that the test files import, by count of files.
fn runner_by_imports(root: &Path, test_files: &[String]) -> Option<Runner> {
    let mut counts = [
        (Runner::Vitest, 0usize),
        (Runner::BunTest, 0),
        (Runner::NodeTest, 0),
    ];
    for file in test_files.iter().take(MAX_SCANNED_TEST_FILES) {
        let Ok(text) = fs::read_to_string(root.join(file)) else {
            continue;
        };
        for (runner, count) in &mut counts {
            if imports_runner(&text, *runner) {
                *count += 1;
            }
        }
    }
    // `max_by_key` keeps the last maximum, so the list goes in reverse
    // priority order.
    counts
        .iter()
        .rev()
        .filter(|(_, count)| *count > 0)
        .max_by_key(|(_, count)| *count)
        .map(|(runner, _)| *runner)
}

/// True when the text imports the test module of the runner, as an import
/// or a `require` call with a quoted specifier.
fn imports_runner(text: &str, runner: Runner) -> bool {
    let specifier = match runner {
        Runner::Vitest => "vitest",
        Runner::NodeTest => "node:test",
        Runner::BunTest => "bun:test",
    };
    ['"', '\''].iter().any(|quote| {
        let quoted = format!("{quote}{specifier}{quote}");
        text.match_indices(&quoted).any(|(index, _)| {
            let before = text[..index].trim_end();
            before.ends_with("from") || before.ends_with("import") || before.ends_with("require(")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("dir");
        for (path, text) in files {
            let target = dir.path().join(path);
            fs::create_dir_all(target.parent().expect("parent")).expect("dir");
            fs::write(target, text).expect("write");
        }
        dir
    }

    fn detect(files: &[(&str, &str)], tests: &[&str]) -> Runner {
        let dir = project(files);
        let tests: Vec<String> = tests.iter().map(|file| (*file).to_owned()).collect();
        select_runner(MutationRunner::Auto, dir.path(), &tests)
    }

    #[test]
    fn an_explicit_config_value_wins_over_detection() {
        let dir = project(&[("test/a.test.ts", "import { test } from \"bun:test\";\n")]);
        let tests = vec!["test/a.test.ts".to_owned()];
        assert_eq!(
            select_runner(MutationRunner::NodeTest, dir.path(), &tests),
            Runner::NodeTest
        );
        assert_eq!(
            select_runner(MutationRunner::Vitest, dir.path(), &tests),
            Runner::Vitest
        );
        assert_eq!(
            select_runner(MutationRunner::Auto, dir.path(), &tests),
            Runner::BunTest
        );
    }

    #[test]
    fn a_node_test_import_selects_node_test() {
        let runner = detect(
            &[
                ("package.json", r#"{"name":"p"}"#),
                (
                    "test/a.test.ts",
                    "import assert from 'node:assert';\nimport { test } from 'node:test';\n",
                ),
            ],
            &["test/a.test.ts"],
        );
        assert_eq!(runner, Runner::NodeTest);
        let required = detect(
            &[(
                "test/a.test.cjs",
                "const { test } = require(\"node:test\");\n",
            )],
            &["test/a.test.cjs"],
        );
        assert_eq!(required, Runner::NodeTest);
    }

    #[test]
    fn a_vitest_dependency_wins_over_the_imports() {
        let runner = detect(
            &[
                ("package.json", r#"{"devDependencies":{"vitest":"^5.0.0"}}"#),
                ("test/a.test.ts", "import { test } from 'node:test';\n"),
            ],
            &["test/a.test.ts"],
        );
        assert_eq!(runner, Runner::Vitest);
    }

    #[test]
    fn the_import_of_the_most_test_files_wins() {
        let runner = detect(
            &[
                ("a.test.ts", "import { test } from 'node:test';\n"),
                ("b.test.ts", "import { test } from 'bun:test';\n"),
                ("c.test.ts", "import { test } from 'bun:test';\n"),
            ],
            &["a.test.ts", "b.test.ts", "c.test.ts"],
        );
        assert_eq!(runner, Runner::BunTest);
        let tie = detect(
            &[
                ("a.test.ts", "import { test } from 'node:test';\n"),
                ("b.test.ts", "import { test } from 'vitest';\n"),
            ],
            &["a.test.ts", "b.test.ts"],
        );
        assert_eq!(tie, Runner::Vitest);
    }

    #[test]
    fn a_bun_root_file_or_a_node_test_script_decides_without_imports() {
        let bun = detect(
            &[("bun.lock", "{}"), ("a.test.ts", "test('x', () => {});\n")],
            &["a.test.ts"],
        );
        assert_eq!(bun, Runner::BunTest);
        let node = detect(
            &[(
                "package.json",
                r#"{"scripts":{"test":"node --test test/"}}"#,
            )],
            &[],
        );
        assert_eq!(node, Runner::NodeTest);
        assert_eq!(detect(&[("package.json", "{}")], &[]), Runner::Vitest);
    }

    #[test]
    fn a_borp_script_selects_node_test_without_imports() {
        for script in [
            "borp -p 'test/**/*.js'",
            "npm run lint && borp --coverage",
            "npx borp",
            "./node_modules/.bin/borp test/a.test.js",
        ] {
            let manifest = format!(r#"{{"scripts":{{"test":{}}}}}"#, serde_json::json!(script));
            let runner = detect(
                &[
                    ("package.json", manifest.as_str()),
                    ("test/a.test.js", "test('x', () => {});\n"),
                ],
                &["test/a.test.js"],
            );
            assert_eq!(runner, Runner::NodeTest, "`{script}`");
        }
        let other = detect(
            &[(
                "package.json",
                r#"{"scripts":{"test":"borpx run","lint":"echo borp-check"}}"#,
            )],
            &[],
        );
        assert_eq!(other, Runner::Vitest);
    }

    #[test]
    fn a_specifier_in_a_string_is_not_an_import() {
        assert!(!imports_runner(
            "const name = \"node:test\";\n",
            Runner::NodeTest
        ));
        assert!(imports_runner(
            "import {\n  test,\n} from \"node:test\";\n",
            Runner::NodeTest
        ));
        assert!(imports_runner(
            "import test from 'node:test';\n",
            Runner::NodeTest
        ));
        assert!(imports_runner("import 'bun:test';\n", Runner::BunTest));
    }
}
