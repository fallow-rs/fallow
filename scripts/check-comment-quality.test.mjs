import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { scanSourceText, scanUnifiedDiff } from "./check-comment-quality.mjs";

const SCRIPT_PATH = resolve(dirname(fileURLToPath(import.meta.url)), "check-comment-quality.mjs");

test("source scan finds high-signal narrator comments", () => {
  const findings = [
    ...scanSourceText("src/main.rs", "// Here we parse the input.\nlet value = parse();\n"),
    ...scanSourceText("scripts/build.py", "# Step 2: build the package\nbuild()\n"),
    ...scanSourceText("src/view.ts", "/* Next we render the result. */\nrender();\n"),
    ...scanSourceText("src/check.ts", "validate(); // Now we report the result.\n"),
    ...scanSourceText("src/lib.rs", "let value: &'a str = input; // Let's return it.\n"),
  ];

  assert.deepEqual(
    findings.map(({ path, line }) => [path, line]),
    [
      ["src/main.rs", 1],
      ["scripts/build.py", 1],
      ["src/view.ts", 1],
      ["src/check.ts", 1],
      ["src/lib.rs", 1],
    ],
  );
});

test("source scan preserves documentation and explanatory comments", () => {
  const source = [
    "/// Here we document public behavior.",
    "//! Now we describe the module contract.",
    "/** Next we document the JavaScript API. */",
    "// Here we keep the branch because Windows reports a different error.",
    "// Step 1 is retained to prevent a protocol downgrade.",
    "// Here we NOTE the compatibility constraint.",
    "// Step 2: fallow-ignore the generated binding.",
    "const text = '// Finally we render';",
    'const text = "prefix // Now we render";',
  ].join("\n");

  assert.deepEqual(scanSourceText("src/lib.rs", source), []);
});

test("source scan ignores narrator-shaped text inside multiline strings", () => {
  const template = [
    "const example = `",
    "// Now we show the example.",
    "`;",
    "// Finally we run the example.",
  ].join("\n");
  const rustRaw = ['let example = r#"', "// Now we show the example.", '"#;', ""].join("\n");
  const pythonTriple = ['example = """', "# Step 2: show the example", '"""', ""].join("\n");

  assert.deepEqual(scanSourceText("src/example.ts", template), [
    { path: "src/example.ts", line: 4, text: "// Finally we run the example." },
  ]);
  assert.deepEqual(scanSourceText("src/example.rs", rustRaw), []);
  assert.deepEqual(scanSourceText("scripts/example.py", pythonTriple), []);
});

test("source scan ignores unsupported and non-comment files", () => {
  assert.deepEqual(scanSourceText("docs/guide.md", "// Here we explain the guide.\n"), []);
  assert.deepEqual(scanSourceText("assets/message.txt", "# First we prepare.\n"), []);
});

test("diff scan reports added line numbers and ignores removed narration", () => {
  const diff = [
    "diff --git a/src/main.rs b/src/main.rs",
    "--- a/src/main.rs",
    "+++ b/src/main.rs",
    "@@ -8,2 +8,3 @@",
    "-// Now we remove the legacy branch.",
    "+// The branch preserves the old wire contract.",
    "+// Finally we return the result.",
    " return result;",
    "diff --git a/docs/guide.md b/docs/guide.md",
    "--- a/docs/guide.md",
    "+++ b/docs/guide.md",
    "@@ -0,0 +1 @@",
    "+// Here we document the workflow.",
  ].join("\n");

  assert.deepEqual(scanUnifiedDiff(diff), [
    {
      path: "src/main.rs",
      line: 9,
      text: "// Finally we return the result.",
    },
  ]);
});

test("diff scan handles new files and multiple hunks", () => {
  const diff = [
    "diff --git a/scripts/task.sh b/scripts/task.sh",
    "new file mode 100755",
    "--- /dev/null",
    "+++ b/scripts/task.sh",
    "@@ -0,0 +1,2 @@",
    "+#!/usr/bin/env bash",
    "+# Let's prepare the release.",
    "@@ -0,0 +20 @@",
    "+# Step 3: publish",
  ].join("\n");

  assert.deepEqual(
    scanUnifiedDiff(diff).map(({ line }) => line),
    [2, 20],
  );
});

test("CLI modes enforce staged and working-tree additions", () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-comment-quality-"));
  const git = (...args) => execFileSync("git", args, { cwd: root, stdio: "ignore" });
  const run = (args, input = "") =>
    spawnSync(process.execPath, [SCRIPT_PATH, ...args], {
      cwd: root,
      encoding: "utf8",
      input,
    });

  try {
    git("init", "--quiet");
    writeFileSync(join(root, "main.rs"), "fn main() {}\n");
    writeFileSync(join(root, "example.ts"), "const example = `\nold value\n`;\n");
    git("add", "main.rs", "example.ts");
    git(
      "-c",
      "user.name=Comment Guard Test",
      "-c",
      "user.email=comment-guard@example.invalid",
      "-c",
      "commit.gpgsign=false",
      "commit",
      "--quiet",
      "--no-verify",
      "-m",
      "initial",
    );

    writeFileSync(join(root, "main.rs"), "// Here we start the program.\nfn main() {}\n");
    writeFileSync(join(root, "example.ts"), "const example = `\n// Now we show the example.\n`;\n");
    git("add", "main.rs", "example.ts");

    const staged = run(["--staged"]);
    assert.equal(staged.status, 1, staged.stderr);
    assert.match(staged.stderr, /main\.rs:1/u);
    assert.doesNotMatch(staged.stderr, /example\.ts/u);

    writeFileSync(join(root, "task.py"), "# Step 2: process the result\n");
    const workingTree = run(
      ["--working-tree", "--claude-hook"],
      JSON.stringify({ stop_hook_active: false }),
    );
    assert.equal(workingTree.status, 2, workingTree.stderr);
    assert.match(workingTree.stderr, /task\.py:1/u);

    const repeatedStop = run(
      ["--working-tree", "--claude-hook"],
      JSON.stringify({ stop_hook_active: true }),
    );
    assert.equal(repeatedStop.status, 0, repeatedStop.stderr);
    assert.equal(repeatedStop.stderr, "");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
