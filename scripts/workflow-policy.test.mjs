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

const listedPaths = (block) =>
  Array.from(block.matchAll(/^\s+- '([^']+)'$/gm), (match) => match[1]);

const matchesListedPath = (patterns, path) =>
  patterns.some((pattern) =>
    pattern.endsWith("/**") ? path.startsWith(pattern.slice(0, -2)) : path === pattern,
  );

test("workflow block parser ignores blank lines and comments before a sibling", () => {
  const source = ["root:", "  value: true", "", "# note", "sibling:", "  value: false"].join("\n");

  assert.equal(indentedBlock(source, "root", 0), "root:\n  value: true\n\n# note");
});

test("workflow block parser rejects missing keys", () => {
  assert.throws(() => indentedBlock("root:\n  value: true", "missing", 0), /missing missing block/);
});

test("bundled skill validation uses the root lockfile without network fallback", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const npmPackageJob = indentedBlock(workflow, "npm-package", 2);
  const rootPackage = JSON.parse(readFileSync("package.json", "utf8"));
  const nestedPackage = JSON.parse(readFileSync("npm/fallow/package.json", "utf8"));
  const lockfile = JSON.parse(readFileSync("package-lock.json", "utf8"));

  assert.equal(rootPackage.devDependencies["@tanstack/intent"], "0.3.5");
  assert.equal(
    nestedPackage.devDependencies?.["@tanstack/intent"],
    rootPackage.devDependencies["@tanstack/intent"],
  );
  assert.equal(lockfile.packages[""].devDependencies["@tanstack/intent"], "0.3.5");
  assert.equal(lockfile.packages["node_modules/@tanstack/intent"].version, "0.3.5");
  assert.match(
    npmPackageJob,
    /npm ci --no-audit --no-fund --ignore-scripts[\s\S]*npx --no-install intent validate npm\/fallow\/skills/,
  );
  assert.doesNotMatch(npmPackageJob, /npx[^\n]*@tanstack\/intent@/);
});

test("CI runs the checked-in Action against the current Rust binary", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const changesJob = indentedBlock(workflow, "changes", 2);
  const actionFilter = listedPaths(indentedBlock(changesJob, "action-current", 12));
  const actionJob = indentedBlock(workflow, "action-current", 2);
  const aggregateJob = indentedBlock(workflow, "ci-ok", 2);
  const publishedCompatibilityWorkflow = readWorkflow(".github/workflows/test-action.yml");

  assert.match(actionJob, /needs: changes/);
  assert.match(actionJob, /if: needs\.changes\.outputs\.action-current == 'true'/);
  assert.match(actionJob, /timeout-minutes: (?:1[0-9]|20)/);
  assert.match(actionJob, /persist-credentials: false/);
  assert.match(actionJob, /uses: \.\/\.github\/actions\/setup-rust/);
  assert.match(actionJob, /cargo build --bin fallow/);
  assert.match(
    actionJob,
    /FALLOW_BIN: \$\{\{ github\.workspace \}\}\/target\/debug\/fallow[\s\S]*bash action\/tests\/run\.sh/,
  );
  assert.match(actionJob, /uses: \.\//);
  assert.match(actionJob, /format: json/);
  assert.match(actionJob, /jq empty "\$RESULTS_PATH"/);
  assert.match(aggregateJob, /action-current/);

  for (const path of [
    "action/**",
    "action.yml",
    "crates/**",
    "Cargo.toml",
    "Cargo.lock",
    ".github/actions/setup-rust/**",
    ".github/workflows/ci.yml",
    "scripts/workflow-policy.test.mjs",
  ]) {
    assert.ok(actionFilter.includes(path), `current-binary Action filter is missing ${path}`);
  }

  assert.match(publishedCompatibilityWorkflow, /uses: \.\//);
  assert.match(publishedCompatibilityWorkflow, /FALLOW_SKIP_BINARY_VERIFY: "1"/);
  assert.doesNotMatch(publishedCompatibilityWorkflow, /cargo build --bin fallow/);
});

test("binary-size workflow isolates incompatible release builds", () => {
  const workflow = readWorkflow(".github/workflows/bloat.yml");
  const globalEnv = indentedBlock(workflow, "env", 0);
  const cliJob = indentedBlock(workflow, "cli-bloat", 2);
  const shippedJob = indentedBlock(workflow, "shipped-binaries", 2);
  const aggregateJob = indentedBlock(workflow, "bloat", 2);

  assert.match(cliJob, /cargo bloat --release -p fallow-cli/);
  assert.match(cliJob, /CARGO_PROFILE_RELEASE_STRIP: "none"/);
  assert.match(cliJob, /CARGO_PROFILE_RELEASE_DEBUG: "2"/);
  assert.doesNotMatch(cliJob, /fallow-lsp|fallow-mcp|fallow-multicall/);
  assert.doesNotMatch(globalEnv, /CARGO_PROFILE_RELEASE_(STRIP|DEBUG)/);
  assert.match(shippedJob, /cargo build --release -p fallow-lsp -p fallow-mcp -p fallow-multicall/);
  assert.doesNotMatch(shippedJob, /cargo bloat/);
  assert.match(aggregateJob, /needs:\n\s+- cli-bloat\n\s+- shipped-binaries/);
  assert.match(aggregateJob, /if: \$\{\{ always\(\) \}\}/);
  assert.match(aggregateJob, /needs\.cli-bloat\.result/);
  assert.match(aggregateJob, /needs\.shipped-binaries\.result/);
  assert.match(aggregateJob, /exit 1/);
  assert.match(aggregateJob, /needs\.cli-bloat\.outputs\.bytes/);
  for (const output of ["lsp_bytes", "mcp_bytes", "multicall_bytes"]) {
    assert.match(aggregateJob, new RegExp(`needs\\.shipped-binaries\\.outputs\\.${output}`));
  }

  for (const job of [cliJob, shippedJob]) {
    const timeout = Number(job.match(/timeout-minutes: (\d+)/)?.[1]);
    assert.ok(
      timeout <= 20,
      `binary build job must fit the 20 minute runner budget, got ${timeout}`,
    );
  }
});

test("regular CI keeps affected checks on Ubuntu", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const windowsRustPaths = listedPaths(indentedBlock(workflow, "windows-rust", 12));
  const checkJob = indentedBlock(workflow, "check", 2);
  const windowsRustJob = indentedBlock(workflow, "windows-rust", 2);
  const zedJob = indentedBlock(workflow, "zed", 2);
  const aggregateJob = indentedBlock(workflow, "ci-ok", 2);
  const workflowWithoutWindowsRust = workflow.replace(windowsRustJob, "");

  assert.doesNotMatch(workflowWithoutWindowsRust, /windows-latest|windows-11-arm|macos-latest/);
  assert.match(checkJob, /runs-on: ubuntu-latest/);
  assert.match(checkJob, /timeout-minutes: 20/);
  assert.doesNotMatch(checkJob, /matrix\.|windows-latest|macos-latest/);
  assert.match(windowsRustJob, /needs: changes/);
  assert.match(windowsRustJob, /if: needs\.changes\.outputs\.windows-rust == 'true'/);
  assert.match(windowsRustJob, /runs-on: windows-latest/);
  assert.ok(windowsRustPaths.includes("crates/core/src/discover/walk.rs"));
  assert.ok(windowsRustPaths.includes("crates/core/src/plugins/manifest_entries.rs"));
  assert.ok(
    windowsRustPaths.includes("crates/core/tests/integration_test/symlink_root_containment.rs"),
  );
  assert.ok(windowsRustPaths.includes("crates/lsp/**"));
  assert.match(windowsRustJob, /cargo test -p fallow-engine changed_files::tests/);
  assert.match(windowsRustJob, /cargo test -p fallow-engine churn::tests/);
  assert.match(windowsRustJob, /cargo test -p fallow-core symlink/);
  assert.match(
    windowsRustJob,
    /^[ \t]+run: cargo test -p fallow-lsp windows_initialization_publishes_uri_safe_diagnostics$/m,
  );
  assert.match(
    windowsRustJob,
    /cargo test -p fallow-mcp completed_success_cleans_descendant_process_tree/,
  );
  assert.match(
    windowsRustJob,
    /^[ \t]+run: cargo clippy -p fallow-core -p fallow-engine -p fallow-lsp -p fallow-mcp --all-targets -- -D warnings$/m,
  );
  assert.match(zedJob, /runs-on: ubuntu-latest/);
  assert.doesNotMatch(zedJob, /matrix\.|windows-latest|macos-latest/);
  assert.throws(() => indentedBlock(workflow, "windows-arm64", 2), /missing windows-arm64 block/);
  assert.throws(
    () => indentedBlock(workflow, "windows-audit-smoke", 2),
    /missing windows-audit-smoke block/,
  );
  assert.match(aggregateJob, /windows-rust/);
  assert.doesNotMatch(aggregateJob, /windows-audit-smoke|windows-arm64/);
});

test("release runs Windows correctness and lifecycle verification without credentials", () => {
  const releaseWorkflow = readWorkflow(".github/workflows/release.yml");
  const validationWorkflow = readWorkflow(".github/workflows/release-validation.yml");
  const job = indentedBlock(validationWorkflow, "windows-verify", 2);
  const buildJob = indentedBlock(releaseWorkflow, "build", 2);

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
  const workflow = readWorkflow(".github/workflows/release-validation.yml");
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

  assert.match(gate, /needs: \[build, validate\]/);
  assert.match(gate, /permissions: \{\}/);
  assert.match(publishCrates, /needs: release-verified/);
  assert.match(release, /needs: release-verified/);
  assert.match(npmPublish, /needs: \[npm-prep, release\]/);
  assert.match(vscodePublish, /needs: \[vscode-prep, release\]/);
});

test("release verifies committed signing-key parity before signing", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const parityStep = workflow.indexOf("- name: Verify binary-signing public key parity");
  const signingStep = workflow.indexOf("- name: Sign raw binaries");

  assert.notEqual(parityStep, -1, "release must verify signing-key parity");
  assert.notEqual(signingStep, -1, "release must sign raw binaries");
  assert.ok(parityStep < signingStep, "release must verify public-key parity before signing");

  const parityBlock = workflow.slice(parityStep, signingStep);
  assert.match(
    parityBlock,
    /ED25519_BINARY_SIGNING_PUBLIC_KEY: \$\{\{ vars\.ED25519_BINARY_SIGNING_PUBLIC_KEY \}\}/u,
  );
  assert.match(parityBlock, /node scripts\/signing-key-parity\.mjs --release-env/u);
  assert.doesNotMatch(parityBlock, /ED25519_BINARY_SIGNING_PRIVATE_KEY|secrets\./u);
});

test("VS Code CI runs the extension-host integration suite with a pinned cached download", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const vscodeJob = indentedBlock(workflow, "vscode", 2);
  const changesJob = indentedBlock(workflow, "changes", 2);
  const vscodeFilter = indentedBlock(changesJob, "vscode", 12);

  assert.match(workflow, /^  pull_request:$/m, "CI must run for pull requests");
  assert.match(vscodeJob, /needs\.changes\.outputs\.vscode == 'true'/);
  assert.match(vscodeJob, /persist-credentials: false/);
  assert.match(vscodeJob, /version: 11\.10\.0/);
  assert.match(vscodeJob, /pnpm audit --prod/);
  assert.match(vscodeFilter, /editors\/vscode\/\*\*/);
  assert.match(vscodeFilter, /\.github\/workflows\/ci\.yml/);
  for (const path of ["crates/**", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml"]) {
    assert.ok(listedPaths(vscodeFilter).includes(path), `VS Code filter is missing ${path}`);
  }
  assert.match(vscodeJob, /uses: \.\/\.github\/actions\/setup-rust/);
  assert.match(
    vscodeJob,
    /name: Build current multicall binary\n\s+run: cargo build -p fallow-multicall --bin fallow-multicall/,
  );
  assert.match(
    vscodeJob,
    /name: Cache VS Code test download[\s\S]*uses: actions\/cache@[0-9a-f]{40}[\s\S]*path: \/tmp\/fallow-vscode-test-cache[\s\S]*key: .*vscode-1\.96\.0/,
  );
  assert.match(
    vscodeJob,
    /name: Run VS Code extension-host integration tests\n\s+run: cd editors\/vscode && xvfb-run -a pnpm test:integration/,
  );
  assert.match(
    vscodeJob,
    /name: Run VS Code real CLI and LSP contract smoke[\s\S]*FALLOW_BIN: \$\{\{ github\.workspace \}\}\/target\/debug\/fallow-multicall[\s\S]*run: xvfb-run -a pnpm --dir editors\/vscode run test:integration:real/,
  );

  const harness = readFileSync("editors/vscode/test/integration/runTest.ts", "utf8");
  const packageJson = readFileSync("editors/vscode/package.json", "utf8");
  assert.match(packageJson, /"packageManager": "pnpm@11\.10\.0"/);
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

test("coverage path filter contains the complete CI Rust contract", () => {
  const coverageWorkflow = readWorkflow(".github/workflows/coverage.yml");
  const coverageJob = indentedBlock(coverageWorkflow, "coverage", 2);
  const coveragePaths = listedPaths(indentedBlock(coverageJob, "rust", 12));
  const ciWorkflow = readWorkflow(".github/workflows/ci.yml");
  const ciChangesJob = indentedBlock(ciWorkflow, "changes", 2);
  const ciRustPaths = listedPaths(indentedBlock(ciChangesJob, "rust", 12));

  assert.match(coverageJob, /dorny\/paths-filter@7b450fff21473bca461d4b92ce414b9d0420d706/);
  for (const path of ciRustPaths) {
    assert.ok(coveragePaths.includes(path), `coverage filter is missing CI Rust path ${path}`);
  }
  for (const path of [
    ".github/actions/setup-rust/**",
    ".github/workflows/ci.yml",
    ".github/workflows/coverage.yml",
    "scripts/workflow-policy.test.mjs",
  ]) {
    assert.ok(coveragePaths.includes(path), `coverage filter is missing policy path ${path}`);
  }
});

test("coverage path filter runs for relevant changes and skips unrelated pull requests", () => {
  const workflow = readWorkflow(".github/workflows/coverage.yml");
  const coverageJob = indentedBlock(workflow, "coverage", 2);
  const coveragePaths = listedPaths(indentedBlock(coverageJob, "rust", 12));

  for (const path of [
    "crates/core/src/lib.rs",
    "tests/fixtures/project/src/index.ts",
    "Cargo.toml",
    "docs/output-schema.json",
    ".github/actions/setup-rust/action.yml",
    ".github/workflows/ci.yml",
    ".github/workflows/coverage.yml",
    "scripts/workflow-policy.test.mjs",
  ]) {
    assert.ok(matchesListedPath(coveragePaths, path), `coverage must run for ${path}`);
  }
  for (const path of ["README.md", "docs/usage.md", "apps/review-electron/src/main/index.ts"]) {
    assert.ok(!matchesListedPath(coveragePaths, path), `coverage must skip ${path}`);
  }
});

test("coverage required check succeeds as a no-op while trusted events still run heavy work", () => {
  const workflow = readWorkflow(".github/workflows/coverage.yml");
  const coverageJob = indentedBlock(workflow, "coverage", 2);

  assert.match(workflow, /^  pull_request:$/m);
  assert.match(workflow, /^  workflow_dispatch:$/m);
  assert.match(coverageJob, /^    name: Coverage$/m);
  assert.match(
    coverageJob,
    /name: Detect coverage-affecting changes[\s\S]*if: github\.event_name == 'pull_request'/,
  );
  assert.match(
    coverageJob,
    /name: Determine whether coverage is required[\s\S]*github\.event_name != 'pull_request'[\s\S]*steps\.coverage_filter\.outputs\.rust == 'true'/,
  );
  assert.match(
    coverageJob,
    /name: Skip coverage for unrelated pull request\n\s+if: steps\.coverage_policy\.outputs\.run != 'true'/,
  );

  for (const name of [
    "Set up Rust",
    "Install cargo-llvm-cov",
    "Build CLI binary for e2e tests",
    "Run tests with coverage",
    "Compute coverage",
    "Enforce coverage floor",
    "Compute badge color",
    "Write coverage metrics",
  ]) {
    assert.match(
      coverageJob,
      new RegExp(
        `name: ${name.replaceAll(/[.*+?^${}()|[\\]\\]/g, "\\$&")}\\n\\s+if: steps\\.coverage_policy\\.outputs\\.run == 'true'`,
      ),
      `${name} must be guarded by the coverage policy`,
    );
  }
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
