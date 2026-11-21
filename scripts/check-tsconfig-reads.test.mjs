import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";

import { findTsconfigReads, scanCrates } from "./check-tsconfig-reads.mjs";
import { makeTempDir } from "./lib/temp-dir.mjs";

const writeFile = (root, relative, contents) => {
  const file = path.join(root, relative);
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, contents);
};

test("a fixture with one bad read fails with its file and line", () => {
  const root = makeTempDir("fallow-tsconfig-reads-");
  writeFile(
    root,
    "crates/core/src/plugins/jsx.rs",
    'fn jsx(json: &Value) -> Option<&Value> {\n    json.get("compilerOptions")?.get("jsx")\n}\n',
  );
  writeFile(
    root,
    "crates/config/src/workspace/tsconfig_merged.rs",
    'fn read(value: &Value) { let _ = value.get("compilerOptions"); }\n',
  );
  writeFile(root, "crates/core/tests/it.rs", 'fn t(v: &Value) { let _ = v.get("compilerOptions"); }\n');
  assert.deepEqual(scanCrates(root), ['crates/core/src/plugins/jsx.rs:2: "compilerOptions"']);

  const cli = spawnSync(process.execPath, ["scripts/check-tsconfig-reads.mjs", "--root", root], {
    encoding: "utf8",
  });
  assert.equal(cli.status, 1);
  assert.match(cli.stderr, /tsconfig-reads: crates\/core\/src\/plugins\/jsx\.rs:2/u);
});

test("each spelling of the key is a read", () => {
  for (const key of ['"compilerOptions"', 'r"compilerOptions"', 'r#"compilerOptions"#']) {
    assert.equal(findTsconfigReads(`let _ = json.get(${key});`).length, 1, key);
  }
  assert.equal(
    findTsconfigReads('let _ = extract(source, path, &["compilerOptions", "jsx"]);').length,
    1,
  );
});

test("comments, larger strings, test items and allowed lines do not count", () => {
  const source = [
    '// json.get("compilerOptions") in a comment',
    'const FIXTURE: &str = r#"{"compilerOptions":{"jsx":"react-jsx"}}"#;',
    'const ESCAPED: &str = "{\\"compilerOptions\\":{}}";',
    "#[cfg(test)]",
    "mod tests {",
    '    fn t(v: &Value) { let _ = v.get("compilerOptions"); }',
    "}",
    "// The finding needs a span of this file. tsconfig-reads: allow",
    'let spans = find_object(obj, "compilerOptions");',
    'fn real(v: &Value) { let _ = v.get("compilerOptions"); }',
  ].join("\n");
  assert.deepEqual(findTsconfigReads(source), [{ line: 10, text: '"compilerOptions"' }]);
});

test("the repository has no direct read", () => {
  const result = spawnSync(process.execPath, ["scripts/check-tsconfig-reads.mjs"], {
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
});
