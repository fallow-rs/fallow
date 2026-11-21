import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";

import { findHostReads, isExempt, scanCrates } from "./check-host-reads.mjs";
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

test("test files and the owner are exempt", () => {
  for (const relative of [
    "crates/process/src/host_paths.rs",
    "crates/cli/src/audit_tests.rs",
    "crates/cli/src/coverage/mod_tests.rs",
    "crates/lsp/src/tests.rs",
    "crates/cli/src/test_support/mod.rs",
  ]) {
    assert.ok(isExempt(relative), relative);
  }
  assert.ok(!isExempt("crates/cli/src/telemetry.rs"));
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
