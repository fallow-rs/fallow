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

test("fuzz workflow runs every harness with bounded scheduled coverage", () => {
  const workflow = readWorkflow(".github/workflows/fuzz-smoke.yml");
  const fuzzManifest = readFileSync("fuzz/Cargo.toml", "utf8");
  const manifestTargets = Array.from(
    fuzzManifest.matchAll(/^\[\[bin\]\]\nname = "([^"]+)"$/gm),
    (match) => match[1],
  );
  const pushPaths = listedPaths(indentedBlock(workflow, "push", 2));
  const pullRequestPaths = listedPaths(indentedBlock(workflow, "pull_request", 2));
  const job = indentedBlock(workflow, "fuzz-smoke", 2);
  const workflowTargets = job
    .match(/targets=\(([^)]+)\)/)?.[1]
    .trim()
    .split(/\s+/);

  assert.notEqual(manifestTargets.length, 0, "fuzz manifest must define targets");
  assert.deepEqual(workflowTargets, manifestTargets);
  assert.match(workflow, /^  schedule:\n    - cron: '30 5 \* \* 0'$/m);
  assert.match(workflow, /FUZZ_TIME_SECONDS:.*'schedule'.*'300'.*'30'/);
  assert.match(workflow, /FUZZ_TARGET_TRIPLE: x86_64-unknown-linux-gnu/);
  assert.match(job, /persist-credentials: false/);
  assert.match(job, /toolchain: nightly-2026-07-20/);
  assert.match(job, /tool: cargo-fuzz@0\.13\.2/);
  assert.match(job, /fallback: cargo-install/);
  assert.match(job, /cargo \+nightly-2026-07-20 metadata --locked/);
  assert.match(job, /set -u/);
  assert.match(job, /for target in "\$\{targets\[@\]\}"/);
  assert.match(
    job,
    /cargo \+nightly-2026-07-20 fuzz run --target "\$FUZZ_TARGET_TRIPLE" "\$target"/,
  );
  assert.match(job, /-max_total_time="\$FUZZ_TIME_SECONDS" -timeout=10/);
  assert.match(job, /if ! cargo[\s\S]*failed=1[\s\S]*exit "\$failed"/);
  assert.match(job, /actions\/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/);
  assert.match(job, /if: failure\(\)[\s\S]*path: fuzz\/artifacts\//);

  for (const path of [
    "fuzz/**",
    "Cargo.toml",
    "Cargo.lock",
    ".cargo/**",
    "crates/extract/**",
    "crates/core/**",
    "crates/types/**",
    "crates/graph/**",
    "crates/config/**",
    "crates/security/**",
    "rust-toolchain.toml",
    ".github/workflows/fuzz-smoke.yml",
  ]) {
    assert.ok(pushPaths.includes(path), `fuzz push filter is missing ${path}`);
    assert.ok(pullRequestPaths.includes(path), `fuzz pull request filter is missing ${path}`);
  }
});

test("bundled skill validation uses the root lockfile without network fallback", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const npmPackageJob = indentedBlock(workflow, "npm-package", 2);
  const rootPackage = JSON.parse(readFileSync("package.json", "utf8"));
  const nestedPackage = JSON.parse(readFileSync("npm/fallow/package.json", "utf8"));
  const lockfile = JSON.parse(readFileSync("package-lock.json", "utf8"));

  assert.equal(rootPackage.devDependencies["@tanstack/intent"], "0.3.6");
  assert.equal(
    nestedPackage.devDependencies?.["@tanstack/intent"],
    rootPackage.devDependencies["@tanstack/intent"],
  );
  assert.equal(lockfile.packages[""].devDependencies["@tanstack/intent"], "0.3.6");
  assert.equal(lockfile.packages["node_modules/@tanstack/intent"].version, "0.3.6");
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

test("Action PR comment smoke requires and verifies the branded GitHub App", () => {
  const workflow = readWorkflow(".github/workflows/test-action.yml");
  const job = indentedBlock(workflow, "test-comment", 2);

  assert.match(job, /permissions:\n\s+contents: read\n\s+id-token: write\n\s+pull-requests: write/);
  assert.match(
    job,
    /if: github\.event_name == 'pull_request' && github\.event\.pull_request\.head\.repo\.full_name == github\.repository/,
  );
  assert.match(job, /EXPECTED_COMMENT_AUTHOR: fallow-cloud\[bot\]/);
  assert.match(job, /"\$COMMENT_COUNT" -ne 1/);
  assert.match(job, /COMMENT_AUTHOR=.*jq -r '\.\[0\]\.user\.login \/\/ empty'/);
  assert.match(job, /"\$COMMENT_AUTHOR" != "\$EXPECTED_COMMENT_AUTHOR"/);
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

test("CodSpeed simulation jobs allow release LTO plus the tracked suites", () => {
  const workflow = readWorkflow(".github/workflows/bench.yml");
  const fastJob = indentedBlock(workflow, "benchmark", 2);
  const fullJob = indentedBlock(workflow, "benchmark-full", 2);

  assert.match(fastJob, /timeout-minutes: 45/);
  assert.match(fullJob, /timeout-minutes: 45/);
});

test("regular CI keeps affected checks on Ubuntu", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const npmPackage = JSON.parse(readFileSync("npm/fallow/package.json", "utf8"));
  const windowsRustPaths = listedPaths(indentedBlock(workflow, "windows-rust", 12));
  const windowsTypeAwarePaths = listedPaths(indentedBlock(workflow, "windows-type-aware", 12));
  const vscodePaths = listedPaths(indentedBlock(workflow, "vscode", 12));
  const checkJob = indentedBlock(workflow, "check", 2);
  const windowsRustJob = indentedBlock(workflow, "windows-rust", 2);
  const windowsTypeAwareJob = indentedBlock(workflow, "windows-type-aware", 2);
  const vscodePackageTargetsJob = indentedBlock(workflow, "vscode-package-targets", 2);
  const vscodeTargetHostJob = indentedBlock(workflow, "vscode-target-host", 2);
  const zedJob = indentedBlock(workflow, "zed", 2);
  const aggregateJob = indentedBlock(workflow, "ci-ok", 2);
  const workflowWithoutWindowsJobs = workflow
    .replace(windowsRustJob, "")
    .replace(windowsTypeAwareJob, "")
    .replace(vscodePackageTargetsJob, "")
    .replace(vscodeTargetHostJob, "");

  assert.doesNotMatch(workflowWithoutWindowsJobs, /windows-latest|windows-11-arm|macos-latest/);
  assert.match(checkJob, /runs-on: ubuntu-latest/);
  assert.match(checkJob, /timeout-minutes: 20/);
  assert.doesNotMatch(checkJob, /matrix\.|windows-latest|macos-latest/);
  assert.match(vscodePackageTargetsJob, /runs-on: ubuntu-latest/);
  assert.match(vscodeTargetHostJob, /linux-x64[\s\S]*win32-x64[\s\S]*darwin-x64/u);
  assert.match(windowsRustJob, /needs: changes/);
  assert.match(windowsRustJob, /if: needs\.changes\.outputs\.windows-rust == 'true'/);
  assert.match(windowsRustJob, /runs-on: windows-latest/);
  assert.ok(windowsRustPaths.includes("crates/core/src/discover/walk.rs"));
  assert.ok(windowsRustPaths.includes("crates/core/src/plugins/manifest_entries.rs"));
  assert.ok(
    windowsRustPaths.includes("crates/core/tests/integration_test/symlink_root_containment.rs"),
  );
  assert.ok(windowsRustPaths.includes("crates/engine/src/repo_refs.rs"));
  assert.ok(windowsRustPaths.includes("crates/cli/src/signal/**"));
  assert.ok(windowsRustPaths.includes("crates/cli/src/type_aware.rs"));
  assert.ok(windowsRustPaths.includes("crates/lsp/**"));
  assert.match(windowsRustJob, /cargo test -p fallow-engine changed_files::tests/);
  assert.match(windowsRustJob, /cargo test -p fallow-engine churn::tests/);
  assert.match(windowsRustJob, /cargo test -p fallow-engine repo_refs::tests/);
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
    /cargo test -p fallow-cli windows_job_object_terminates_descendants_without_taskkill_lookup/,
  );
  assert.match(
    windowsRustJob,
    /^[ \t]+run: cargo clippy -p fallow-cli -p fallow-core -p fallow-engine -p fallow-lsp -p fallow-mcp --all-targets -- -D warnings$/m,
  );
  assert.match(windowsTypeAwareJob, /needs: changes/);
  assert.match(windowsTypeAwareJob, /if: needs\.changes\.outputs\.windows-type-aware == 'true'/);
  assert.match(windowsTypeAwareJob, /runs-on: windows-latest/);
  assert.ok(windowsTypeAwarePaths.includes("npm/fallow/scripts/**"));
  assert.equal(npmPackage.scripts.test, "node --test scripts/*.test.js");
  assert.match(windowsTypeAwareJob, /npm --prefix npm\/fallow test/);
  assert.ok(windowsTypeAwarePaths.includes("npm/fallow/package.json"));
  assert.ok(windowsTypeAwarePaths.includes("npm/fallow/bin/**"));
  assert.ok(windowsTypeAwarePaths.includes("tools/type-aware-sidecar/**"));
  assert.ok(windowsTypeAwarePaths.includes("editors/vscode/scripts/package-type-aware.mjs"));
  assert.ok(
    windowsTypeAwarePaths.includes("editors/vscode/scripts/verify-packaged-type-aware.mjs"),
  );
  assert.ok(windowsTypeAwarePaths.includes("crates/api/src/type_aware/transport/**"));
  assert.match(windowsTypeAwareJob, /cargo test -p fallow-api type_aware::transport/);
  assert.match(windowsTypeAwareJob, /type-aware-windows-candidate-smoke\.mjs/);
  assert.doesNotMatch(windowsTypeAwareJob, /pnpm package|verify:vsix|FALLOW_EXTENSION_PATH/);
  assert.match(vscodePackageTargetsJob, /package:variants/);
  assert.match(vscodePackageTargetsJob, /test:packaging/);
  assert.ok(vscodePaths.includes(".github/workflows/release.yml"));
  assert.ok(vscodePaths.includes(".github/workflows/release-validation.yml"));
  assert.match(vscodeTargetHostJob, /verify:vsix/);
  assert.match(vscodeTargetHostJob, /FALLOW_EXTENSION_PATH=/);
  assert.match(vscodeTargetHostJob, /FALLOW_BIN:/);
  assert.match(vscodeTargetHostJob, /FALLOW_LSP_BIN:/);
  assert.match(vscodeTargetHostJob, /name: Run exact target VSIX host smoke/);
  assert.match(zedJob, /runs-on: ubuntu-latest/);
  assert.doesNotMatch(zedJob, /matrix\.|windows-latest|macos-latest/);
  assert.throws(() => indentedBlock(workflow, "windows-arm64", 2), /missing windows-arm64 block/);
  assert.throws(
    () => indentedBlock(workflow, "windows-audit-smoke", 2),
    /missing windows-audit-smoke block/,
  );
  assert.match(aggregateJob, /windows-rust/);
  assert.match(aggregateJob, /windows-type-aware/);
  assert.match(aggregateJob, /needs: \[[^\n]*\bzed\b[^\n]*\]/);
  assert.doesNotMatch(aggregateJob, /windows-audit-smoke|windows-arm64/);
});

test("MSRV CI forces Rust 1.92 despite the repository toolchain override", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const msrvJob = indentedBlock(workflow, "msrv", 2);

  assert.match(msrvJob, /RUSTUP_TOOLCHAIN: 1\.92\.0/);
  assert.match(msrvJob, /toolchain: '1\.92\.0'/);
  assert.match(msrvJob, /run: cargo check --workspace/);
});

test("Rust walltime benchmarks use CodSpeed macro runners", () => {
  const workflow = readWorkflow(".github/workflows/bench-rust-walltime.yml");
  const cargoManifest = readFileSync("Cargo.toml", "utf8");
  const job = indentedBlock(workflow, "benchmark", 2);

  assert.match(workflow, /^  workflow_dispatch:/m);
  assert.doesNotMatch(workflow, /^  (push|pull_request|schedule):/m);
  assert.match(job, /runs-on: codspeed-macro/);
  assert.match(job, /permissions:\n\s+contents: read\n\s+id-token: write/);
  assert.match(job, /cargo codspeed build -m walltime/);
  assert.match(job, /cargo codspeed run -m walltime/);
  assert.match(job, /mode: walltime/);
  assert.doesNotMatch(workflow, /--cfg codspeed/);
  assert.match(
    cargoManifest,
    /criterion = \{ package = "codspeed-criterion-compat", version = "=4\.7\.0"/,
  );
  assert.match(job, /tool: cargo-codspeed@4\.7\.0/);
  assert.match(job, /case "\$WALLTIME_WORKLOAD" in/);
  assert.match(workflow, /- component-config/);
  assert.match(job, /component-config\)[\s\S]*bench=component_config/);
  assert.match(workflow, /- component-output/);
  assert.match(job, /component-output\)[\s\S]*bench=component_output/);
  assert.match(workflow, /- dupes-detect/);
  assert.match(job, /dupes-detect\)[\s\S]*package=fallow-engine[\s\S]*bench=dupes_detect/);
  assert.match(workflow, /- representative-sources/);
  assert.match(
    job,
    /representative-sources\)[\s\S]*package=fallow-benchmarks[\s\S]*bench=representative_sources/,
  );
  assert.doesNotMatch(job, /run:.*\$\{\{ inputs\.workload \}\}/);
});

test("release runs Windows correctness and lifecycle verification without credentials", () => {
  const releaseWorkflow = readWorkflow(".github/workflows/release.yml");
  const validationWorkflow = readWorkflow(".github/workflows/release-validation.yml");
  const job = indentedBlock(validationWorkflow, "windows-verify", 2);
  const vscodePackageJob = indentedBlock(validationWorkflow, "vscode-package-targets", 2);
  const vscodeTargetJob = indentedBlock(validationWorkflow, "vscode-target-host", 2);
  const buildJob = indentedBlock(releaseWorkflow, "build", 2);

  assert.match(buildJob, /target: x86_64-pc-windows-msvc/);
  assert.match(buildJob, /target: aarch64-pc-windows-msvc/);
  assert.match(buildJob, /os: windows-11-arm/);
  assert.match(job, /runs-on: windows-latest/);
  assert.match(job, /permissions:\n\s+contents: read/);
  assert.doesNotMatch(job, /id-token: write|contents: write|secrets\./);
  assert.match(job, /npm --prefix npm\/fallow test/);
  assert.match(
    job,
    /name: Install type-aware sidecar dependencies[\s\S]*npm ci --prefix tools\/type-aware-sidecar --no-audit --no-fund --ignore-scripts[\s\S]*name: Run workspace tests/,
  );
  assert.match(job, /cargo test --workspace --lib --bins --tests --examples/);
  assert.match(job, /cargo clippy --workspace --all-targets -- -D warnings/);
  assert.match(job, /cargo fmt --all -- --check/);
  assert.match(job, /npm run publish:prepare/);
  assert.match(job, /cd crates\/napi && npm test/);
  assert.match(vscodeTargetJob, /verify:vsix/);
  assert.match(vscodeTargetJob, /FALLOW_LSP_BIN:/);
  assert.match(vscodePackageJob, /name: validation-vscode-targets/);
  assert.match(vscodePackageJob, /test:packaging/);
  assert.doesNotMatch(vscodePackageJob, /name: fallow-vscode-targets/);
  assert.match(job, /type-aware-windows-candidate-smoke\.mjs/);
  assert.match(job, /FALLOW_CANDIDATE_BIN:/);
  assert.match(job, /audit_orphan_sweep_removes_dead_pid_worktree/);
  assert.match(job, /run_fallow_timeout_terminates_and_reaps_windows_job_tree/);
});

test("NAPI builds preserve the maintained native loader", () => {
  const packageJson = JSON.parse(readFileSync("crates/napi/package.json", "utf8"));
  for (const script of [packageJson.scripts.build, packageJson.scripts["build:debug"]]) {
    assert.match(script, /\bnapi build\b/u);
    assert.match(script, /--no-js\b/u);
  }

  for (const path of [
    ".github/workflows/ci.yml",
    ".github/workflows/release.yml",
    ".github/workflows/release-validation.yml",
  ]) {
    const commands = [...readWorkflow(path).matchAll(/\bnpx napi build[^\n]*/gu)].map(
      (match) => match[0],
    );
    assert.ok(commands.length > 0, `${path} must build the NAPI addon`);
    for (const command of commands) {
      assert.match(command, /--no-js\b/u, `${path}: ${command}`);
    }
  }
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
  const context = indentedBlock(workflow, "release-context", 2);
  const build = indentedBlock(workflow, "build", 2);
  const validate = indentedBlock(workflow, "validate", 2);
  const gate = indentedBlock(workflow, "release-verified", 2);
  const publishCrates = indentedBlock(workflow, "publish-crates", 2);
  const releaseAssets = indentedBlock(workflow, "release-assets", 2);
  const releaseReady = indentedBlock(workflow, "release-ready", 2);
  const npmPublish = indentedBlock(workflow, "npm-publish", 2);
  const vscodePrep = indentedBlock(workflow, "vscode-prep", 2);
  const vscodeHostSmoke = indentedBlock(workflow, "vscode-host-smoke", 2);
  const vscodeMarketplace = indentedBlock(workflow, "vscode-publish-marketplace", 2);
  const vscodeOpenVsx = indentedBlock(workflow, "vscode-publish-open-vsx", 2);
  const vscodePublicVerify = indentedBlock(workflow, "vscode-public-verify", 2);
  const vscodePackage = JSON.parse(readFileSync("editors/vscode/package.json", "utf8"));

  assert.match(context, /permissions:\n\s+contents: read/);
  assert.doesNotMatch(context, /^\s+\w+: write$/mu);
  // `administration` is not a grantable GITHUB_TOKEN scope; declaring it makes
  // the workflow unparseable and every dispatch fails with HTTP 422.
  assert.doesNotMatch(workflow, /^\s+administration:/mu);
  assert.match(build, /needs: release-context/);
  assert.match(validate, /needs: release-context/);
  assert.match(gate, /needs: \[build, validate\]/);
  assert.match(gate, /permissions: \{\}/);
  assert.match(publishCrates, /needs: \[release-verified, release-assets\]/);
  assert.match(releaseAssets, /needs: \[release-verified, vscode-prep, vscode-host-smoke\]/);
  assert.match(releaseAssets, /permissions:\n\s+contents: read/);
  assert.match(releaseAssets, /pattern: fallow-\*/);
  assert.match(npmPublish, /needs: \[npm-prep, release-assets\]/);
  assert.match(vscodePrep, /package:variants --/);
  assert.match(vscodePrep, /fallow-vscode-targets/);
  assert.match(vscodePrep, /targets=\(\s+universal/su);
  assert.match(vscodePrep, /inventory\.json SHA256SUMS/);
  assert.match(vscodePrep, /\.entries\[\].*\.targetPlatform/su);
  assert.match(vscodeHostSmoke, /needs: vscode-prep/);
  assert.match(vscodeHostSmoke, /linux-x64[\s\S]*win32-x64[\s\S]*darwin-x64/u);
  assert.match(vscodeHostSmoke, /name: fallow-vscode-targets/);
  assert.match(vscodeHostSmoke, /name: fallow-cli-\$\{\{ matrix\.npm_dir \}\}/);
  assert.match(vscodeHostSmoke, /name: fallow-lsp-\$\{\{ matrix\.npm_dir \}\}/);
  assert.match(vscodeHostSmoke, /fallow-vscode-\$version-\$FALLOW_VSIX_TARGET\.vsix/);
  assert.match(vscodeHostSmoke, /verify:vsix[\s\S]*--target[\s\S]*--version/u);
  assert.match(vscodeHostSmoke, /FALLOW_EXTENSION_PATH=/);
  assert.match(vscodeHostSmoke, /FALLOW_BIN=/);
  assert.match(vscodeHostSmoke, /FALLOW_LSP_BIN=/);
  assert.match(vscodeHostSmoke, /chmod \+x/);
  assert.match(vscodeHostSmoke, /unzip -q/);
  assert.match(vscodeHostSmoke, /test:integration:real/);
  assert.match(vscodeHostSmoke, /persist-credentials: false/);
  assert.doesNotMatch(vscodeHostSmoke, /secrets\.|cargo (?:build|install)/u);

  for (const [job, registry, cli, pin, secret, otherSecret] of [
    [
      vscodeMarketplace,
      "VS Code Marketplace",
      "@vscode/vsce",
      vscodePackage.devDependencies["@vscode/vsce"],
      "VSCE_PAT",
      "OVSX_PAT",
    ],
    [vscodeOpenVsx, "Open VSX", "ovsx", vscodePackage.devDependencies.ovsx, "OVSX_PAT", "VSCE_PAT"],
  ]) {
    assert.match(job, /needs: \[vscode-prep, vscode-host-smoke, release-assets\]/, registry);
    assert.match(job, /permissions: \{\}/, registry);
    assert.match(
      job,
      new RegExp(
        `npm install -g --ignore-scripts ${cli.replace("/", "\\/")}@${pin.replaceAll(".", "\\.")}`,
        "u",
      ),
      registry,
    );
    assert.match(job, new RegExp(`secrets\\.${secret}`, "u"), registry);
    assert.doesNotMatch(job, new RegExp(`secrets\\.${otherSecret}`, "u"), registry);
    assert.doesNotMatch(
      job,
      /actions\/checkout|pnpm|npm (?:ci|install)(?! -g)|\bbuild\b/u,
      registry,
    );
    assert.doesNotMatch(job, /continue-on-error/u, registry);
    assert.match(job, /\.entries\[\]\.file/u, registry);
    assert.match(job, /--skip-duplicate/u, registry);
    assert.match(job, /failed=1[\s\S]*exit "\$failed"/u, registry);
  }

  assert.match(
    vscodePublicVerify,
    /needs: \[vscode-prep, vscode-publish-marketplace, vscode-publish-open-vsx\]/,
  );
  assert.match(vscodePublicVerify, /persist-credentials: false/);
  assert.match(vscodePublicVerify, /node scripts\/vscode-public-verify\.mjs --artifact-dir/);
  assert.doesNotMatch(vscodePublicVerify, /secrets\.|_PAT|npm install|pnpm install/u);
  assert.match(
    releaseReady,
    /needs: \[publish-crates, npm-publish, vscode-public-verify, release-assets\]/,
  );
  assert.match(releaseReady, /permissions:\n\s+contents: read/);
  assert.match(releaseReady, /Release tag .* appeared before the release workflow completed/u);
});

test("release keeps the version tag last and requires curated public notes", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const context = indentedBlock(workflow, "release-context", 2);
  const releaseAssets = indentedBlock(workflow, "release-assets", 2);
  const skill = readFileSync(".agents/skills/release/SKILL.md", "utf8");
  const downloadStep = releaseAssets.indexOf("- name: Download all artifacts");
  const absentTagStep = releaseAssets.indexOf("- name: Reconfirm release tag is absent");
  const assembleStep = releaseAssets.indexOf("- name: Assemble release asset bundle");
  const uploadStep = releaseAssets.indexOf("- name: Upload release asset bundle");
  const workflowDispatch = skill.indexOf("gh workflow run release.yml");
  const downloadBundle = skill.indexOf('gh run download "$RUN_ID"');
  const signedTag = skill.indexOf('git tag -s "$TAG"');
  const createRelease = skill.indexOf('gh release create "$TAG"');

  assert.notEqual(downloadStep, -1, "release must download every built artifact");
  assert.notEqual(absentTagStep, -1, "release must reconfirm tag absence");
  assert.notEqual(assembleStep, -1, "release must assemble the final asset bundle");
  assert.notEqual(uploadStep, -1, "release must store the final asset bundle");
  assert.ok(downloadStep < absentTagStep);
  assert.ok(absentTagStep < assembleStep);
  assert.ok(assembleStep < uploadStep);
  assert.match(workflow, /^  workflow_dispatch:$/mu);
  assert.match(workflow, /^\s{6}tag:$/mu);
  assert.doesNotMatch(workflow, /^\s{6}release_id:$/mu);
  assert.doesNotMatch(workflow, /^  push:\n\s+tags:/mu);
  assert.doesNotMatch(workflow, /github\.ref_name|refs\/tags\/v/mu);
  assert.match(context, /GITHUB_REF.*refs\/heads\/main/su);
  assert.match(context, /Release tag must match vMAJOR\.MINOR\.PATCH/u);
  // The immutability gate lives in the maintainer flow: the endpoint is not
  // readable with any grantable GITHUB_TOKEN scope.
  assert.doesNotMatch(context, /\$\{GITHUB_REPOSITORY\}\/immutable-releases/u);
  assert.match(skill, /immutable-releases/u);
  assert.match(skill, /Release immutability is not enabled/u);
  assert.match(context, /Release tag .* already exists; tag creation must remain near the end/u);
  assert.match(releaseAssets, /Release tag .* appeared before publication completed/u);
  assert.match(releaseAssets, /No release assets were downloaded/u);
  assert.match(releaseAssets, /Duplicate release asset name/u);
  assert.match(releaseAssets, /name: release-assets/u);
  assert.match(releaseAssets, /if-no-files-found: error/u);
  assert.match(releaseAssets, /retention-days: 7/u);
  assert.doesNotMatch(
    workflow,
    /gh release create|softprops\/action-gh-release|git tag|git push origin/u,
  );

  assert.match(skill, /Draft curated public GitHub release notes before starting the publication/u);
  assert.match(skill, /exact full-changelog comparison URL/u);
  assert.match(skill, /non-empty body/u);
  assert.match(skill, /release title must be/u);
  assert.match(skill, /release title or notes contain an em-dash/u);
  assert.match(skill, /name no competing or upstream third-party project/u);
  assert.match(skill, /--name release-assets/u);
  assert.match(skill, /--verify-tag/u);
  assert.match(skill, /--notes-file "\$NOTES_FILE"/u);
  assert.notEqual(workflowDispatch, -1, "skill must dispatch the release workflow");
  assert.notEqual(downloadBundle, -1, "skill must download the exact run asset bundle");
  assert.notEqual(signedTag, -1, "skill must create a signed release tag");
  assert.notEqual(createRelease, -1, "skill must create the immutable release");
  assert.ok(workflowDispatch < downloadBundle, "workflow must complete before asset download");
  assert.ok(downloadBundle < signedTag, "asset bundle must exist before tag creation");
  assert.ok(signedTag < createRelease, "signed tag must exist before release creation");
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

  assert.match(coverageJob, /dorny\/paths-filter@ceb8a2b8f2d89434be7ff52d3de7ec3738c5cc9d/);
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
    "Set up Node.js",
    "Install type-aware sidecar dependencies",
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
