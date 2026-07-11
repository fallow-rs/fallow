import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const readWorkflow = (path) => readFileSync(path, "utf8");

const isIgnoredLine = (line) => line.trim() === "" || line.trimStart().startsWith("#");

const indentationOf = (line) => line.length - line.trimStart().length;

const isBlockBoundary = (line, indent) => !isIgnoredLine(line) && indentationOf(line) <= indent;

const findBlockEnd = (lines, start, indent) => {
  const relativeEnd = lines.slice(start + 1).findIndex((line) => isBlockBoundary(line, indent));
  return relativeEnd === -1 ? lines.length : start + 1 + relativeEnd;
};

const indentedBlock = (source, key, indent) => {
  const lines = source.split(/\r?\n/);
  const prefix = " ".repeat(indent);
  const start = lines.findIndex((line) => line === `${prefix}${key}:`);
  assert.notEqual(start, -1, `missing ${key} block`);
  const end = findBlockEnd(lines, start, indent);
  return lines.slice(start, end).join("\n");
};

test("workflow block parser ignores blank lines and comments before a sibling", () => {
  const source = ["root:", "  value: true", "", "# note", "sibling:", "  value: false"].join("\n");

  assert.equal(indentedBlock(source, "root", 0), "root:\n  value: true\n\n# note");
});

test("workflow block parser rejects missing keys", () => {
  assert.throws(() => indentedBlock("root:\n  value: true", "missing", 0), /missing missing block/);
});

test("regular CI keeps affected checks on Ubuntu", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const checkJob = indentedBlock(workflow, "check", 2);
  const zedJob = indentedBlock(workflow, "zed", 2);
  const aggregateJob = indentedBlock(workflow, "ci-ok", 2);

  assert.doesNotMatch(workflow, /windows-latest|windows-11-arm|macos-latest/);
  assert.match(checkJob, /runs-on: ubuntu-latest/);
  assert.match(checkJob, /timeout-minutes: 20/);
  assert.doesNotMatch(checkJob, /matrix\.|windows-latest|macos-latest/);
  assert.match(zedJob, /runs-on: ubuntu-latest/);
  assert.doesNotMatch(zedJob, /matrix\.|windows-latest|macos-latest/);
  assert.throws(() => indentedBlock(workflow, "windows-arm64", 2), /missing windows-arm64 block/);
  assert.throws(
    () => indentedBlock(workflow, "windows-audit-smoke", 2),
    /missing windows-audit-smoke block/,
  );
  assert.doesNotMatch(aggregateJob, /windows-audit-smoke|windows-arm64/);
});

test("release runs Windows correctness and lifecycle verification without credentials", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const job = indentedBlock(workflow, "windows-verify", 2);
  const buildJob = indentedBlock(workflow, "build", 2);

  assert.match(buildJob, /target: x86_64-pc-windows-msvc/);
  assert.match(buildJob, /target: aarch64-pc-windows-msvc/);
  assert.match(buildJob, /os: windows-11-arm/);
  assert.match(job, /runs-on: windows-latest/);
  assert.match(job, /permissions:\n\s+contents: read/);
  assert.doesNotMatch(job, /id-token: write|contents: write|secrets\./);
  assert.match(job, /cargo test --workspace --lib --bins --tests --examples/);
  assert.match(job, /cargo clippy --workspace --all-targets -- -D warnings/);
  assert.match(job, /cargo fmt --all -- --check/);
  assert.match(job, /npm run publish:prepare/);
  assert.match(job, /cd crates\/napi && npm test/);
  assert.match(job, /audit_orphan_sweep_removes_dead_pid_worktree/);
  assert.match(job, /run_fallow_timeout_terminates_and_reaps_windows_job_tree/);
});

test("release runs Zed verification on macOS and Windows without credentials", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const job = indentedBlock(workflow, "zed-verify", 2);

  assert.match(job, /os: \[macos-latest, windows-latest\]/);
  assert.match(job, /permissions:\n\s+contents: read/);
  assert.doesNotMatch(job, /id-token: write|contents: write|secrets\./);
  assert.match(job, /cargo test --manifest-path editors\/zed\/Cargo.toml/);
  assert.match(job, /cargo build --target wasm32-wasip2 --manifest-path editors\/zed\/Cargo.toml/);
  assert.match(job, /cargo fmt --check --manifest-path editors\/zed\/Cargo.toml/);
});

test("release publication waits for the aggregate verification gate", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const gate = indentedBlock(workflow, "release-verified", 2);
  const publishCrates = indentedBlock(workflow, "publish-crates", 2);
  const release = indentedBlock(workflow, "release", 2);
  const npmPublish = indentedBlock(workflow, "npm-publish", 2);
  const vscodePublish = indentedBlock(workflow, "vscode-publish", 2);

  assert.match(gate, /needs: \[build, check-codegen, windows-verify, zed-verify\]/);
  assert.match(gate, /permissions: \{\}/);
  assert.match(publishCrates, /needs: release-verified/);
  assert.match(release, /needs: release-verified/);
  assert.match(npmPublish, /needs: \[npm-prep, release\]/);
  assert.match(vscodePublish, /needs: \[vscode-prep, release\]/);
});

test("VS Code CI runs the extension-host integration suite with a pinned cached download", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const vscodeJob = indentedBlock(workflow, "vscode", 2);
  const changesJob = indentedBlock(workflow, "changes", 2);
  const vscodeFilter = indentedBlock(changesJob, "vscode", 12);

  assert.match(workflow, /^  pull_request:$/m, "CI must run for pull requests");
  assert.match(vscodeJob, /needs\.changes\.outputs\.vscode == 'true'/);
  assert.match(vscodeJob, /persist-credentials: false/);
  assert.match(vscodeFilter, /editors\/vscode\/\*\*/);
  assert.match(vscodeFilter, /\.github\/workflows\/ci\.yml/);
  assert.match(
    vscodeJob,
    /name: Cache VS Code test download[\s\S]*uses: actions\/cache@[0-9a-f]{40}[\s\S]*path: \/tmp\/fallow-vscode-test-cache[\s\S]*key: .*vscode-1\.96\.0/,
  );
  assert.match(
    vscodeJob,
    /name: Run VS Code extension-host integration tests\n\s+run: cd editors\/vscode && xvfb-run -a pnpm test:integration/,
  );

  const harness = readFileSync("editors/vscode/test/integration/runTest.ts", "utf8");
  assert.match(harness, /version: "1\.96\.0"/);
});

test("coverage floor runs with read-only permissions on pull requests and pushes", () => {
  const workflow = readWorkflow(".github/workflows/coverage.yml");
  const coverageJob = indentedBlock(workflow, "coverage", 2);
  const pushTrigger = indentedBlock(workflow, "push", 2);

  assert.match(workflow, /^  pull_request:$/m, "coverage must run for pull requests");
  assert.match(workflow, /^  push:$/m, "coverage must run for pushes");
  assert.match(pushTrigger, /branches: \[main\]/);
  assert.match(coverageJob, /permissions:\n\s+contents: read/);
  assert.match(coverageJob, /persist-credentials: false/);
  assert.match(coverageJob, /name: Enforce coverage floor/);
  assert.match(
    coverageJob,
    /name: Upload coverage publication input\n\s+if: >-[\s\S]*github\.event_name == 'push'[\s\S]*github\.ref == 'refs\/heads\/main'[\s\S]*github\.event_name == 'workflow_dispatch'/,
  );
  assert.match(coverageJob, /badge_color: \$\{\{ steps\.badge\.outputs\.color \}\}/);
  assert.doesNotMatch(coverageJob, /name: Store coverage metrics/);
  assert.doesNotMatch(coverageJob, /name: Update coverage badge/);
});

test("coverage publication is isolated to trusted events and write permissions", () => {
  const workflow = readWorkflow(".github/workflows/coverage.yml");
  const publishJob = indentedBlock(workflow, "publish", 2);

  assert.match(publishJob, /permissions:\n\s+contents: write/);
  assert.match(publishJob, /needs: coverage/);
  assert.match(publishJob, /github\.event_name == 'push'/);
  assert.match(publishJob, /github\.ref == 'refs\/heads\/main'/);
  assert.match(publishJob, /github\.event_name == 'workflow_dispatch'/);
  assert.match(publishJob, /BADGE_COLOR: \$\{\{ needs\.coverage\.outputs\.badge_color \}\}/);
  assert.match(publishJob, /name: Store coverage metrics/);
  assert.match(publishJob, /name: Update coverage badge/);
  assert.doesNotMatch(publishJob, /\b(?:cargo|npm|pnpm)\b/);
});
