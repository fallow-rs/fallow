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
    { path: "src/example.ts", line: 4, text: "// Finally we run the example.", kind: "narration" },
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
      kind: "narration",
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

test("source scan finds comments that describe the edit history", () => {
  const source = [
    "// This used to be a HashMap; now it is a Vec.",
    "/// Previously the parser returned `None` here.",
    "//! Each module used to be a test binary of its own.",
    "/** The old behavior skipped hidden files. */",
    " * The old logic dropped hidden files.",
    "# Keeps the previous behavior for callers.",
    "// Added as requested in review.",
    "// Changed per the review feedback.",
    "// A test like this exists to keep behavior; this fix exists to remove it.",
    "// Gates before this change printed the report.",
    "// #2685: the gate used to be exit 2, which hid the security verdict.",
    "// This used to panic on empty input.",
    "// Previously, a missing file panicked.",
    "// After this change, the cache is shared.",
    "// The previous implementation cloned the map.",
    "// Originally this returned a Vec.",
    "// Renamed as per review.",
  ].join("\n");

  assert.deepEqual(
    scanSourceText("src/lib.rs", source).map(({ line, kind }) => [line, kind]),
    [
      [1, "history"],
      [2, "history"],
      [3, "history"],
      [4, "history"],
      [5, "history"],
      [6, "history"],
      [7, "history"],
      [8, "history"],
      [9, "history"],
      [10, "history"],
      [11, "history"],
      [12, "history"],
      [13, "history"],
      [14, "history"],
      [15, "history"],
      [16, "history"],
      [17, "history"],
    ],
  );
});

test("history scan keeps domain wording, issue context, and directives", () => {
  const source = [
    "/// Per-function CRAP data used to emit `--max-crap` findings.",
    "/// Used to emit the `owners` metadata for a rule.",
    "/// Whether `fallow fix` can apply this fix automatically.",
    "/// Current total after the change.",
    "//! Reports whether the scope is applied as requested by the caller.",
    "// No rename in this change, so no previous signal id.",
    "/// Fail when the change introduces a new security-sink candidate.",
    "// The branch preserves the old wire contract.",
    "# Remove the baseline entries of findings that this commit fixes.",
    "// When this PR introduces a new dependency, the brief asks about it.",
    "/// Compares the old version of a file in the diff with the new one.",
    "/// The baseline keeps the existing behavior of the base branch.",
    "// Tracks whether the visibility changed from public to private.",
    "// The writer used to emit the report path.",
    "/// The value is used to be compatible with older readers.",
    "const text = '// This used to be a map';",
  ].join("\n");

  assert.deepEqual(scanSourceText("src/lib.rs", source), []);
});

test("pre-tool mode blocks an edit that adds a history comment", () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-comment-pre-tool-"));
  const run = (toolName, toolInput) =>
    spawnSync(process.execPath, [SCRIPT_PATH, "--claude-pre-tool"], {
      cwd: root,
      encoding: "utf8",
      input: JSON.stringify({ tool_name: toolName, tool_input: toolInput }),
    });

  try {
    execFileSync("git", ["init", "--quiet"], { cwd: root });
    writeFileSync(join(root, ".gitignore"), "scratch/\n");
    const lib = join(root, "lib.rs");
    writeFileSync(lib, "// x used to be 2.\nlet x = 1;\nlet y = 1;\n");

    const blocked = run("Edit", {
      file_path: lib,
      old_string: "let y = 1;",
      new_string: "// y used to be 2.\nlet y = 1;",
    });
    assert.equal(blocked.status, 2, blocked.stderr);
    assert.match(blocked.stderr, /y used to be 2/u);
    assert.match(blocked.stderr, /reader/u);

    const nearOldComment = run("Edit", {
      file_path: lib,
      old_string: "let x = 1;",
      new_string: "let x = 3;",
    });
    assert.equal(nearOldComment.status, 0, nearOldComment.stderr);

    const overwrite = run("Write", {
      file_path: lib,
      content: "// x used to be 2.\nlet x = 5;\n",
    });
    assert.equal(overwrite.status, 0, overwrite.stderr);

    const multiEdit = run("MultiEdit", {
      file_path: lib,
      edits: [
        { old_string: "let x = 1;", new_string: "let x = 2;" },
        { old_string: "let y = 1;", new_string: "// Let's set y.\nlet y = 2;" },
      ],
    });
    assert.equal(multiEdit.status, 2, multiEdit.stderr);

    const fixture = join(root, "fixture.rs");
    writeFileSync(fixture, 'let source = r#"\nexport const a = 1;\n"#;\n');
    const insideRawString = run("Edit", {
      file_path: fixture,
      old_string: "export const a = 1;",
      new_string: "// Let's keep the old behavior here\nexport const a = 1;",
    });
    assert.equal(insideRawString.status, 0, insideRawString.stderr);

    const ignored = run("Write", {
      file_path: join(root, "scratch", "probe.rs"),
      content: "// This used to be a struct.\n",
    });
    assert.equal(ignored.status, 0, ignored.stderr);

    const outside = run("Write", {
      file_path: join(tmpdir(), "outside-probe.rs"),
      content: "// This used to be a struct.\n",
    });
    assert.equal(outside.status, 0, outside.stderr);

    const created = run("Write", {
      file_path: join(root, "new.rs"),
      content: "// This used to be a struct.\n",
    });
    assert.equal(created.status, 2, created.stderr);

    const markdown = run("Write", {
      file_path: join(root, "guide.md"),
      content: "// used to be here\n",
    });
    assert.equal(markdown.status, 0, markdown.stderr);

    const malformed = spawnSync(process.execPath, [SCRIPT_PATH, "--claude-pre-tool"], {
      cwd: root,
      encoding: "utf8",
      input: "not json",
    });
    assert.equal(malformed.status, 0, malformed.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a backtick in a regex literal, string, or comment does not open a template", () => {
  const source = [
    "const fence = /`{3}/u;",
    "const quote = '`';",
    'const other = "a ` b";',
    "// Wrap the name in a ` pair.",
    "const ratio = total / count; const tick = /`/u;",
    "/* a ` b */ const done = true;",
    "// This used to be a map.",
    "const template = `",
    "// Here we show the example.",
    "`;",
    "// Previously the loop ran twice.",
  ].join("\n");

  assert.deepEqual(
    scanSourceText("scripts/check.mjs", source).map(({ line }) => line),
    [7, 11],
  );
});
