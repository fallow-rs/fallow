import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";

import {
  findHostReads,
  isTestPredicate,
  moduleDeclarations,
  scanCrates,
  testOnlyFiles,
} from "./check-host-reads.mjs";
import { makeTempDir } from "./lib/temp-dir.mjs";

const writeFile = (root, relative, contents) => {
  const file = path.join(root, relative);
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, contents);
};

test("a fixture with one bad read fails with its file and line", () => {
  const root = makeTempDir("fallow-host-reads-");
  writeFile(
    root,
    "crates/cli/src/config.rs",
    'fn base() -> Option<std::ffi::OsString> {\n    std::env::var_os("HOME")\n}\n',
  );
  writeFile(
    root,
    "crates/process/src/host_paths.rs",
    'fn read() { let _ = std::env::var_os("HOME"); }\n',
  );
  writeFile(root, "crates/cli/tests/it.rs", 'fn t() { let _ = std::env::var_os("PATH"); }\n');
  assert.deepEqual(scanCrates(root), ['crates/cli/src/config.rs:2: var_os("HOME")']);
});

test("each host variable and home_dir is a read", () => {
  const names = ["HOME", "PATH", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "XDG_CONFIG_HOME"];
  for (const name of names) {
    assert.equal(findHostReads(`let _ = std::env::var("${name}");`).length, 1, name);
  }
  assert.equal(findHostReads("let _ = dirs::home_dir();").length, 1);
  assert.equal(findHostReads('let _ = std::env::var_os("FALLOW_COV_BIN");').length, 0);
});

test("reads in comments, strings and test items do not count", () => {
  const source = [
    '// std::env::var_os("HOME") in a comment',
    'const DOC: &str = "std::env::var_os(\\"PATH\\")";',
    "#[cfg(test)]",
    "mod tests {",
    '    fn t() { let _ = std::env::var_os("HOME"); }',
    "}",
    "#[cfg(test)]",
    "fn helper() {",
    '    let _ = std::env::var_os("PATH");',
    "}",
    'fn real() { let _ = std::env::var_os("PATH"); }',
  ].join("\n");
  assert.deepEqual(findHostReads(source), [{ line: 11, text: 'var_os("PATH")' }]);
});

test("a cfg(test) use item does not hide the code after it", () => {
  const source = [
    "#[cfg(test)]",
    "use std::{env, path};",
    'fn real() { let _ = std::env::var_os("HOME"); }',
  ].join("\n");
  assert.deepEqual(findHostReads(source), [{ line: 3, text: 'var_os("HOME")' }]);
});

test("a file is test-only when its parent declares it under a test cfg", () => {
  const sources = new Map([
    ["crates/cli/src/lib.rs", "#[cfg(test)]\nmod audit_tests;\nmod rule_pack;\nmod telemetry;\n"],
    ["crates/cli/src/audit_tests.rs", "mod helpers;\n"],
    ["crates/cli/src/audit_tests/helpers.rs", ""],
    ["crates/cli/src/rule_pack/mod.rs", "mod test;\n"],
    ["crates/cli/src/rule_pack/test.rs", ""],
    [
      "crates/cli/src/telemetry.rs",
      '#[cfg(all(test, not(miri)))]\n#[path = "telemetry_tests.rs"]\nmod tests;\n',
    ],
    ["crates/cli/src/telemetry_tests.rs", ""],
  ]);
  assert.deepEqual([...testOnlyFiles(sources)].toSorted(), [
    "crates/cli/src/audit_tests.rs",
    "crates/cli/src/audit_tests/helpers.rs",
    "crates/cli/src/telemetry_tests.rs",
  ]);
});

test("the guard scans a production module named test.rs", () => {
  const root = makeTempDir("fallow-host-reads-rule-pack-");
  writeFile(root, "crates/cli/src/lib.rs", "mod rule_pack;\n");
  writeFile(root, "crates/cli/src/rule_pack/mod.rs", "mod test;\n");
  writeFile(
    root,
    "crates/cli/src/rule_pack/test.rs",
    'fn run() { let _ = std::env::var_os("PATH"); }\n',
  );
  assert.deepEqual(scanCrates(root), ['crates/cli/src/rule_pack/test.rs:1: var_os("PATH")']);
});

test("the guard scans the fuzz targets", () => {
  const root = makeTempDir("fallow-host-reads-fuzz-");
  writeFile(root, "fuzz/fuzz_targets/fuzz_one.rs", 'fn f() { let _ = std::env::var("HOME"); }\n');
  assert.deepEqual(scanCrates(root), ['fuzz/fuzz_targets/fuzz_one.rs:1: var("HOME")']);
});

test("module declarations keep their path attribute and test cfg", () => {
  const source = '#[cfg(test)]\n#[path = "a_tests.rs"]\nmod tests;\npub(crate) mod real;\n';
  assert.deepEqual(moduleDeclarations(source), [
    { name: "tests", path: "a_tests.rs", testOnly: true },
    { name: "real", path: null, testOnly: false },
  ]);
});

test("test cfg predicates", () => {
  for (const predicate of [
    "test",
    "all(test, not(miri))",
    "all(unix, test)",
    'any(test, feature = "testing")',
  ]) {
    assert.ok(isTestPredicate(predicate), predicate);
  }
  for (const predicate of ["not(test)", "unix", "any(test, unix)", 'feature = "testing"']) {
    assert.ok(!isTestPredicate(predicate), predicate);
  }
});

test("each other read form is a read", () => {
  const forms = {
    alias: 'use std::env::var_os as host_var;\nfn f() { let _ = host_var("HOME"); }',
    "grouped alias": 'use std::env::{var as read};\nfn f() { let _ = read("PATH"); }',
    "env macro": 'const H: &str = env!("HOME");',
    "option_env macro": 'const H: Option<&str> = option_env!("HOME");',
    "raw string key": 'fn f() { let _ = std::env::var_os(r"HOME"); }',
    "raw string key with hashes": 'fn f() { let _ = std::env::var_os(r#"XDG_CONFIG_HOME"#); }',
    turbofish: 'fn f() { let _ = std::env::var_os::<&str>("PATH"); }',
    "vars_os call": "fn f() { for _ in std::env::vars_os() {} }",
    "vars call": "fn f() { for _ in std::env::vars() {} }",
  };
  for (const [form, source] of Object.entries(forms)) {
    assert.equal(findHostReads(source).length, 1, form);
  }
});

test("an allow marker on the line or the line above hides one read", () => {
  const source = [
    "// host-reads: allow: reads the variable names only.",
    "fn names() { for _ in std::env::vars_os() {} }",
    'fn same() { let _ = std::env::var_os("HOME"); } // host-reads: allow: test',
    'fn real() { let _ = std::env::var_os("HOME"); }',
  ].join("\n");
  assert.deepEqual(findHostReads(source), [{ line: 4, text: 'var_os("HOME")' }]);
});

test("the script started as a process fails on the fixture with a bad read", () => {
  const root = makeTempDir("fallow-host-reads-start-");
  writeFile(root, "crates/cli/src/home.rs", 'fn h() { let _ = std::env::var("HOME"); }\n');
  const script = path.join(import.meta.dirname, "check-host-reads.mjs");
  const bad = spawnSync(process.execPath, [script, "--root", root], { encoding: "utf8" });
  assert.equal(bad.status, 1, bad.stderr);
  assert.match(bad.stderr, /host-reads: crates\/cli\/src\/home\.rs:1: var\("HOME"\)/u);

  const repository = spawnSync(process.execPath, [script], { encoding: "utf8" });
  assert.equal(repository.status, 0, repository.stderr);
  assert.match(repository.stdout, /host read check passed/u);
});

test("other test-only cfg forms and item kinds hide their reads", () => {
  const source = [
    "#[cfg(all(unix, test))]",
    'mod unix_tests { fn t() { let _ = std::env::var_os("HOME"); } }',
    '#[cfg(any(test, feature = "testing"))]',
    'pub async fn support() { let _ = std::env::var_os("PATH"); }',
    "#[cfg(test)]",
    'const fn helper() { let _ = std::env::var_os("HOME"); }',
  ].join("\n");
  assert.deepEqual(findHostReads(source), []);
});

test("a not(test) item is production code", () => {
  const source = [
    "#[cfg(not(test))]",
    "fn system_temp_dirs() {",
    '    let _ = std::env::var_os("HOME");',
    "}",
  ].join("\n");
  assert.deepEqual(findHostReads(source), [{ line: 3, text: 'var_os("HOME")' }]);
});
