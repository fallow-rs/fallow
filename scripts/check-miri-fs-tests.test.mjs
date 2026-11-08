import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { test } from "node:test";

import { findMissingMiriIgnores, main, scanFile } from "./check-miri-fs-tests.mjs";

const TEMPDIR_ATTRIBUTE = '#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]';
const FS_ATTRIBUTE = '#[cfg_attr(miri, ignore = "the filesystem is blocked by Miri isolation")]';

const testsOf = (source) =>
  scanFile(source)
    .tests.filter((entry) => entry.missingIgnore)
    .map(({ name, line, pattern }) => ({ name, line, pattern }));

test("a test that calls tempdir without a Miri mark is reported", () => {
  const source = [
    "#[cfg(test)]",
    "mod tests {",
    "    #[test]",
    "    fn writes_a_file() {",
    "        let dir = tempfile::tempdir().unwrap();",
    "    }",
    "}",
  ].join("\n");
  assert.deepEqual(testsOf(source), [{ name: "writes_a_file", line: 4, pattern: "tempdir(" }]);
});

test("each file system pattern of a test body is reported", () => {
  const bodies = {
    "tempfile::": "let f = tempfile::NamedTempFile::new();",
    TempDir: "let d: Option<TempDir> = None;",
    "std::fs::": 'std::fs::write("a", "b").unwrap();',
  };
  for (const [pattern, body] of Object.entries(bodies)) {
    const source = `#[test]\nfn uses_fs() {\n    ${body}\n}\n`;
    assert.deepEqual(testsOf(source), [{ name: "uses_fs", line: 2, pattern }], pattern);
  }
});

test("a test with a Miri ignore mark or a plain ignore is not reported", () => {
  for (const mark of [
    "#[cfg_attr(miri, ignore)]",
    TEMPDIR_ATTRIBUTE,
    '#[cfg_attr( miri , ignore = "slow" )]',
    "#[ignore]",
  ]) {
    const before = `${mark}\n#[test]\nfn a() {\n    let _ = tempfile::tempdir();\n}\n`;
    const after = `#[test]\n${mark}\nfn a() {\n    let _ = tempfile::tempdir();\n}\n`;
    assert.deepEqual(testsOf(before), [], mark);
    assert.deepEqual(testsOf(after), [], mark);
  }
});

test("a pattern in a comment or a string literal is not a file system call", () => {
  const source = [
    "#[test]",
    "fn only_text() {",
    "    // calls tempdir() in a comment",
    '    let text = "std::fs::read and TempDir";',
    '    let raw = r#"tempfile::tempdir()"#;',
    "}",
  ].join("\n");
  assert.deepEqual(testsOf(source), []);
});

test("a brace in a char, byte or block comment literal does not end the test body", () => {
  const source = [
    "#[test]",
    "fn braces<'a>(text: &'a str) {",
    "    let open = '{';",
    "    let close = b'}';",
    "    let escaped = '\\'';",
    '    let bytes = b"}";',
    '    let raw = br#"}"#;',
    "    /* } */",
    "    let _ = tempfile::tempdir();",
    "}",
  ].join("\n");
  assert.deepEqual(testsOf(source), [{ name: "braces", line: 2, pattern: "tempdir(" }]);
});

test("a helper function that is not a test is not reported", () => {
  const source = "fn helper() -> tempfile::TempDir {\n    tempfile::tempdir().unwrap()\n}\n";
  assert.deepEqual(testsOf(source), []);
});

test("a test that Miri does not compile is not reported", () => {
  const gatedModule = [
    "#[cfg(all(test, not(miri)))]",
    "mod tests {",
    "    #[test]",
    "    fn a() { let _ = tempfile::tempdir(); }",
    "}",
    "#[test]",
    "fn b() { let _ = 1; }",
  ].join("\n");
  const gatedTest = "#[cfg(not(miri))]\n#[test]\nfn a() { let _ = tempfile::tempdir(); }\n";
  const gatedFile = "#![cfg(not(miri))]\n#[test]\nfn a() { let _ = tempfile::tempdir(); }\n";
  assert.deepEqual(testsOf(gatedModule), []);
  assert.deepEqual(testsOf(gatedTest), []);
  assert.deepEqual(testsOf(gatedFile), []);
});

test("the report names the file, the line, and the attribute to add", () => {
  const findings = findMissingMiriIgnores([
    {
      path: "crates/demo/src/lib.rs",
      source: '#[test]\nfn a() {\n    std::fs::read("x").ok();\n}\n',
      gated: false,
    },
  ]);
  assert.equal(findings.length, 1);
  assert.match(findings[0].message, /^crates\/demo\/src\/lib\.rs:2: test `a` calls `std::fs::`/u);
  assert.ok(findings[0].message.includes(FS_ATTRIBUTE), findings[0].message);
});

const writeTree = (root, files) => {
  for (const [relative, content] of Object.entries(files)) {
    const absolute = path.join(root, relative);
    mkdirSync(path.dirname(absolute), { recursive: true });
    writeFileSync(absolute, content);
  }
};

const runMain = (root) => {
  const errors = [];
  const logs = [];
  const originalError = console.error;
  const originalLog = console.log;
  console.error = (...parts) => errors.push(parts.join(" "));
  console.log = (...parts) => logs.push(parts.join(" "));
  try {
    return { code: main([], root), errors, logs };
  } finally {
    console.error = originalError;
    console.log = originalLog;
  }
};

test("the check follows module files of each Miri package and skips not(miri) modules", (t) => {
  const root = mkdtempSync(path.join(tmpdir(), "miri-fs-tests-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const unmarked = "#[test]\nfn writes() {\n    let _ = tempfile::tempdir();\n}\n";
  writeTree(root, {
    ".github/workflows/ci.yml": "run: cargo +nightly miri test -p demo --lib --tests\n",
    "crates/demo/Cargo.toml": '[package]\nname = "demo"\n',
    "crates/demo/src/lib.rs": [
      "mod reached;",
      '#[path = "renamed_file.rs"]',
      "mod renamed;",
      "#[cfg(not(miri))]",
      "mod skipped;",
      "mod nested {",
      "    mod inner;",
      "}",
    ].join("\n"),
    "crates/demo/src/reached.rs": unmarked,
    "crates/demo/src/renamed_file.rs": unmarked,
    "crates/demo/src/skipped.rs": unmarked,
    "crates/demo/src/skipped/child.rs": unmarked,
    "crates/demo/src/nested/inner.rs": unmarked,
    "crates/demo/tests/integration.rs": unmarked,
    "crates/other/Cargo.toml": '[package]\nname = "other"\n',
    "crates/other/src/lib.rs": unmarked,
  });

  const failing = runMain(root);
  assert.equal(failing.code, 1);
  const reported = failing.errors.map((line) => line.split(":")[1]).toSorted();
  assert.deepEqual(reported, [
    " crates/demo/src/nested/inner.rs",
    " crates/demo/src/reached.rs",
    " crates/demo/src/renamed_file.rs",
    " crates/demo/tests/integration.rs",
  ]);
  assert.ok(failing.errors.every((line) => line.includes(TEMPDIR_ATTRIBUTE)));

  const marked = `${TEMPDIR_ATTRIBUTE}\n${unmarked}`;
  writeTree(root, {
    "crates/demo/src/reached.rs": marked,
    "crates/demo/src/renamed_file.rs": marked,
    "crates/demo/src/nested/inner.rs": marked,
    "crates/demo/tests/integration.rs": marked,
  });
  const passing = runMain(root);
  assert.equal(passing.code, 0, passing.errors.join("\n"));
});

test("the check fails when the workflow names no Miri package", (t) => {
  const root = mkdtempSync(path.join(tmpdir(), "miri-fs-tests-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  writeTree(root, { ".github/workflows/ci.yml": "jobs: {}\n" });
  const result = runMain(root);
  assert.equal(result.code, 1);
  assert.match(result.errors.join("\n"), /no `miri test -p <crate>` step/u);
});

test("every file system test of the current Miri packages has a Miri mark", () => {
  const result = runMain(path.resolve(import.meta.dirname, ".."));
  assert.equal(result.code, 0, result.errors.join("\n"));
});

test("the script runs as a command and reports the result", () => {
  const result = spawnSync(
    process.execPath,
    [path.join(import.meta.dirname, "check-miri-fs-tests.mjs")],
    {
      encoding: "utf8",
    },
  );
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /miri file system test check passed \(fallow-types/u);
});
