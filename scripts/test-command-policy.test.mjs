import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ciWorkflowRelativePath = ".github/workflows/ci.yml";
const requiredPolicyPaths = [
  "CLAUDE.md",
  ".claude/hooks/pre-bash-guard.py",
  ".githooks/pre-push",
  ciWorkflowRelativePath,
  "scripts/scaffold-analyzer-plan.mjs",
];
const optionalPolicyPaths = [
  ".codex/references/quality-gates.md",
  ".codex/hooks/pre-bash-guard.py",
];
const fullValidationPaths = [
  "CLAUDE.md",
  ".githooks/pre-push",
  "scripts/scaffold-analyzer-plan.mjs",
  ".codex/references/quality-gates.md",
];

const normalTestCommand = "cargo test --workspace --lib --bins --tests --examples";
const benchCompileCommand = "cargo check --workspace --benches";

const existingPaths = (root, paths) =>
  paths.map((relativePath) => join(root, relativePath)).filter(existsSync);

const policyPaths = (root) => [
  ...requiredPolicyPaths.map((relativePath) => join(root, relativePath)),
  ...existingPaths(root, optionalPolicyPaths),
];

const validationPaths = (root) => existingPaths(root, fullValidationPaths);

const leadingSpaces = (line) => line.length - line.trimStart().length;
const isBlank = (line) => line.trim() === "";

const parseRunLine = (line) => {
  const match = line.match(/^(\s*)(?:-\s+)?run:\s*(.*?)\s*$/u);
  return match ? { indent: match[1].length, value: match[2] } : null;
};

const blockStyle = (value) => value.match(/^([|>])[+-]?(?:\s+#.*)?$/u)?.[1] ?? null;

const isBlockLine = (line, runIndent) => isBlank(line) || leadingSpaces(line) > runIndent;

const collectBlockLines = (lines, startIndex, runIndent) => {
  const relativeEnd = lines.slice(startIndex).findIndex((line) => !isBlockLine(line, runIndent));
  const endIndex = relativeEnd === -1 ? lines.length : startIndex + relativeEnd;
  return { lines: lines.slice(startIndex, endIndex), nextIndex: endIndex };
};

const minimumContentIndent = (lines) =>
  lines.reduce(
    (minimum, line) => (isBlank(line) ? minimum : Math.min(minimum, leadingSpaces(line))),
    Number.POSITIVE_INFINITY,
  );

const stripCommonIndent = (lines) => {
  const indent = minimumContentIndent(lines);
  return lines.map((line) => line.slice(Math.min(indent, line.length)));
};

const renderBlockScalar = (lines, style) => {
  const separator = style === ">" ? " " : "\n";
  return stripCommonIndent(lines).join(separator).trim();
};

const runCommandAt = (lines, index) => {
  const run = parseRunLine(lines[index]);
  if (!run) {
    return { command: null, nextIndex: index + 1 };
  }

  const style = blockStyle(run.value);
  if (!style) {
    return { command: run.value || null, nextIndex: index + 1 };
  }

  const block = collectBlockLines(lines, index + 1, run.indent);
  return {
    command: renderBlockScalar(block.lines, style),
    nextIndex: block.nextIndex,
  };
};

const yamlRunCommands = (text) => {
  const lines = text.split("\n");
  const commands = [];
  let index = 0;
  while (index < lines.length) {
    const parsed = runCommandAt(lines, index);
    if (parsed.command) {
      commands.push(parsed.command);
    }
    index = parsed.nextIndex;
  }

  return commands;
};

const assertCiWorkspaceTestCommands = (text) => {
  const workspaceTestCommands = yamlRunCommands(text).filter(
    (command) => command.includes("cargo test") && command.includes("--workspace"),
  );

  assert.notEqual(workspaceTestCommands.length, 0, "CI must execute workspace tests");
  for (const command of workspaceTestCommands) {
    assert.equal(command, normalTestCommand, `unsafe CI workspace test command: ${command}`);
  }
};

test("tracked command-policy files are present", () => {
  for (const filePath of requiredPolicyPaths.map((relativePath) => join(repoRoot, relativePath))) {
    assert.equal(existsSync(filePath), true, `missing tracked policy file: ${filePath}`);
  }
});

test("normal test guidance never executes benchmark targets", () => {
  for (const filePath of policyPaths(repoRoot)) {
    const lines = readFileSync(filePath, "utf8").split("\n");
    const benchmarkRunningLines = lines.filter(
      (line) => line.includes("cargo test") && line.includes("--all-targets"),
    );
    assert.deepEqual(
      benchmarkRunningLines,
      [],
      `${relative(repoRoot, filePath)} must not recommend benchmark-running tests`,
    );
  }
});

test("normal test guidance selects only non-benchmark targets", () => {
  for (const filePath of policyPaths(repoRoot)) {
    if (relative(repoRoot, filePath) === ciWorkflowRelativePath) {
      continue;
    }
    assert.match(
      readFileSync(filePath, "utf8"),
      new RegExp(normalTestCommand),
      `${relative(repoRoot, filePath)} must use the normal non-benchmark target set`,
    );
  }
});

test("CI executable workspace tests use the normal non-benchmark target set", () => {
  const ciWorkflowPath = join(repoRoot, ciWorkflowRelativePath);
  assertCiWorkspaceTestCommands(readFileSync(ciWorkflowPath, "utf8"));
});

test("CI executable policy rejects unsafe multiline workspace tests", () => {
  for (const blockHeader of ["|", ">-"]) {
    const mixedShapeWorkflow = `steps:
  - name: Safe inline test
    run: ${normalTestCommand}
  - name: Unsafe multiline test
    run: ${blockHeader}
      cargo test --workspace --all-targets
`;

    assert.throws(
      () => assertCiWorkspaceTestCommands(mixedShapeWorkflow),
      /unsafe CI workspace test command: cargo test --workspace --all-targets/u,
    );
  }
});

test("full validation compiles benchmarks separately", () => {
  for (const filePath of validationPaths(repoRoot)) {
    assert.match(
      readFileSync(filePath, "utf8"),
      new RegExp(benchCompileCommand),
      `${relative(repoRoot, filePath)} must compile benchmarks explicitly`,
    );
  }
});

test("benchmark test guidance is compile-only", () => {
  for (const filePath of policyPaths(repoRoot)) {
    const lines = readFileSync(filePath, "utf8").split("\n");
    const benchmarkTestLines = lines.filter(
      (line) =>
        line.includes("cargo test") && line.includes("--benches") && !line.includes("--no-run"),
    );
    assert.deepEqual(
      benchmarkTestLines,
      [],
      `${relative(repoRoot, filePath)} must never execute benchmark targets for coverage`,
    );
  }
});

test("optional Codex policy files may be absent in a clean checkout", (t) => {
  const root = mkdtempSync(join(tmpdir(), "fallow-command-policy-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));

  for (const relativePath of requiredPolicyPaths) {
    mkdirSync(dirname(join(root, relativePath)), { recursive: true });
    const command = fullValidationPaths.includes(relativePath)
      ? `${normalTestCommand}\n${benchCompileCommand}\n`
      : `${normalTestCommand}\n`;
    writeFileSync(join(root, relativePath), command);
  }

  assert.deepEqual(
    policyPaths(root).map((filePath) => relative(root, filePath)),
    requiredPolicyPaths,
  );
  assert.deepEqual(
    validationPaths(root).map((filePath) => relative(root, filePath)),
    ["CLAUDE.md", ".githooks/pre-push", "scripts/scaffold-analyzer-plan.mjs"],
  );
});
