//! The project template and the edit steps of the result cache model.
//!
//! One project runs under Vitest and under `bun test`: the test file imports
//! from `vitest`, and Bun maps that import to `bun:test`. The test checks the
//! exact sum only in the strict mode. The Vitest setup file and the Bun
//! preload file each set the mode. A helper that the test imports, and an
//! installed package, can each turn the exact check off. So most steps turn
//! the arithmetic mutant of `add` from killed to survived, or back. A warm
//! run that reuses a stale result then differs from the cache-free run. The
//! tsconfig, touch and `@types/bun` steps change no status: they only vary
//! the key inputs before a later step. The read-set oracle checks the key
//! parts that no step can show.

use std::fs;
use std::path::Path;

/// A test runner of the template, with its `mutation.runner` config value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    Vitest,
    Bun,
}

impl Runner {
    pub const fn config_value(self) -> &'static str {
        match self {
            Self::Vitest => "vitest",
            Self::Bun => "bun-test",
        }
    }
}

const SOURCE: &str = "export const add = (a: number, b: number): number => a + b;\n";

const TEST: &str = r#"import { expect, test } from "vitest";
import { add } from "../src/math";
import { installed } from "mode-pkg";
import { exact } from "./helper";

const mode = (globalThis as { mode?: string }).mode;

test("adds", () => {
  const sum = add(2, 3);
  if (exact && installed && mode === "strict") expect(sum).toBe(5);
  else expect(typeof sum).toBe("number");
});
"#;

const FILES: [(&str, &str); 15] = [
    (
        "package.json",
        "{\"name\":\"result-cache-model\",\"private\":true,\"type\":\"module\",\"devDependencies\":{\"@types/bun\":\"1.3.0\"},\"dependencies\":{\"mode-pkg\":\"1.0.0\"}}\n",
    ),
    (
        "package-lock.json",
        "{\"name\":\"result-cache-model\",\"lockfileVersion\":3,\"packages\":{\"node_modules/mode-pkg\":{\"version\":\"1.0.0\"}}}\n",
    ),
    (
        "node_modules/mode-pkg/package.json",
        "{\"name\":\"mode-pkg\",\"version\":\"1.0.0\",\"type\":\"module\",\"main\":\"index.js\"}\n",
    ),
    (
        "node_modules/mode-pkg/index.js",
        "export const installed = true;\n",
    ),
    (".gitignore", "node_modules\n.fallow\n"),
    ("README.md", "# Result cache model\n"),
    (
        "vitest.config.mjs",
        "export default { test: { include: [\"test/**/*.test.ts\"], setupFiles: [\"./test/setup.ts\"] } };\n",
    ),
    ("bunfig.toml", "[test]\npreload = [\"./test/preload.ts\"]\n"),
    (
        "tsconfig.json",
        "{ \"extends\": \"./tsconfig.base.json\" }\n",
    ),
    (
        "tsconfig.base.json",
        "{ \"compilerOptions\": { \"strict\": true } }\n",
    ),
    ("src/math.ts", SOURCE),
    ("test/math.test.ts", TEST),
    ("test/helper.ts", "export const exact = true;\n"),
    (
        "test/setup.ts",
        "(globalThis as { mode?: string }).mode = \"strict\";\n",
    ),
    (
        "test/preload.ts",
        "(globalThis as { mode?: string }).mode = \"strict\";\n",
    ),
];

/// The files that `bun test` reads in native code. The preload trace sees
/// only the modules that Bun loads, so the read-set oracle adds these.
pub const BUN_NATIVE_READS: [&str; 4] = [
    "bunfig.toml",
    "package.json",
    "tsconfig.json",
    "tsconfig.base.json",
];

/// Writes the template for `runner` to `root`. The Vitest package of the
/// companion comes in through a `node_modules` link.
pub fn write(root: &Path, runner: Runner, vitest: &Path) {
    for (path, content) in FILES {
        let target = root.join(path);
        fs::create_dir_all(target.parent().expect("parent")).expect("dir");
        fs::write(&target, content).expect("write");
    }
    fs::write(root.join(".fallowrc.json"), runner_config(runner)).expect("write");
    std::os::unix::fs::symlink(vitest, root.join("node_modules/vitest")).expect("symlink");
}

fn runner_config(runner: Runner) -> String {
    format!(
        "{{ \"mutation\": {{ \"runner\": \"{}\" }} }}\n",
        runner.config_value()
    )
}

/// One edit of the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The Vitest setup file sets the strict or the loose mode.
    ToggleSetupMode,
    /// The Bun preload file sets the strict or the loose mode (#3473).
    TogglePreloadMode,
    /// The helper that the test imports turns the exact check on or off.
    ToggleHelperExact,
    /// The Vitest config lists the setup file, or no setup file.
    ToggleSetupListed,
    /// `bunfig.toml` lists the preload file, or no preload file.
    TogglePreloadListed,
    /// The Fallow config selects the other runner (#3473).
    SwitchRunner,
    /// The base config of the `tsconfig.json` extends chain changes.
    ToggleTsconfigBase,
    /// The setup file gets a new modification time and the same content.
    TouchSetup,
    /// A new version of the installed package changes it and the lockfile.
    ReinstallPackage,
    /// `package.json` lists `@types/bun`, which enables the Bun plugin of
    /// the analysis, or no such package.
    ToggleBunTypes,
}

impl Step {
    pub const ALL: [Self; 10] = [
        Self::ToggleSetupMode,
        Self::TogglePreloadMode,
        Self::ToggleHelperExact,
        Self::ToggleSetupListed,
        Self::TogglePreloadListed,
        Self::SwitchRunner,
        Self::ToggleTsconfigBase,
        Self::TouchSetup,
        Self::ReinstallPackage,
        Self::ToggleBunTypes,
    ];
}

/// Replaces `a` with `b` in `path`, or `b` with `a` when `a` is absent.
fn swap(root: &Path, path: &str, a: &str, b: &str) {
    let target = root.join(path);
    let text = fs::read_to_string(&target).expect("read");
    let next = if text.contains(a) {
        text.replacen(a, b, 1)
    } else {
        assert!(text.contains(b), "{path} has neither {a:?} nor {b:?}");
        text.replacen(b, a, 1)
    };
    fs::write(&target, next).expect("write");
}

/// Applies `step` to the project at `root`.
pub fn apply(root: &Path, step: Step) {
    match step {
        Step::ToggleSetupMode => swap(root, "test/setup.ts", "\"strict\"", "\"loose\""),
        Step::TogglePreloadMode => swap(root, "test/preload.ts", "\"strict\"", "\"loose\""),
        Step::ToggleHelperExact => swap(root, "test/helper.ts", "true", "false"),
        Step::ToggleSetupListed => swap(
            root,
            "vitest.config.mjs",
            "setupFiles: [\"./test/setup.ts\"]",
            "setupFiles: []",
        ),
        Step::TogglePreloadListed => swap(
            root,
            "bunfig.toml",
            "preload = [\"./test/preload.ts\"]",
            "preload = []",
        ),
        Step::SwitchRunner => swap(
            root,
            ".fallowrc.json",
            "\"vitest\"",
            &format!("\"{}\"", Runner::Bun.config_value()),
        ),
        Step::ToggleTsconfigBase => swap(
            root,
            "tsconfig.base.json",
            "\"strict\": true",
            "\"strict\": false",
        ),
        Step::ReinstallPackage => {
            swap(root, "node_modules/mode-pkg/index.js", "true", "false");
            swap(root, "package-lock.json", "\"1.0.0\"", "\"1.0.1\"");
        }
        Step::ToggleBunTypes => swap(
            root,
            "package.json",
            "\"devDependencies\":{\"@types/bun\":\"1.3.0\"}",
            "\"devDependencies\":{}",
        ),
        Step::TouchSetup => {
            let target = root.join("test/setup.ts");
            let text = fs::read(&target).expect("read");
            fs::write(&target, text).expect("write");
        }
    }
}

/// An edit of `path` that changes its bytes and keeps its meaning: a
/// comment, or white space in a JSON file.
pub fn append_neutral_edit(root: &Path, path: &str) {
    let suffix = match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("ts" | "mts" | "cts" | "tsx" | "js" | "mjs" | "cjs" | "jsx") => {
            "\n// result cache model edit\n"
        }
        Some("toml" | "md") => "\n# result cache model edit\n",
        Some("json") => "\n",
        _ => panic!("the read-set oracle has no neutral edit for {path}"),
    };
    let target = root.join(path);
    let mut text = fs::read_to_string(&target).expect("read");
    text.push_str(suffix);
    fs::write(&target, text).expect("write");
}
