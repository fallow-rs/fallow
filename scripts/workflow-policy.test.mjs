import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
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
  const job = indentedBlock(workflow, "fuzz-smoke", 2);
  const workflowTargets = job
    .match(/targets=\(([^)]+)\)/)?.[1]
    .trim()
    .split(/\s+/);

  assert.notEqual(manifestTargets.length, 0, "fuzz manifest must define targets");
  assert.deepEqual(workflowTargets, manifestTargets);
  assert.match(workflow, /^  schedule:\n    - cron: '30 5 \* \* 0'$/m);
  assert.doesNotMatch(workflow, /^  pull_request:/m, "fuzz runs on main, not on pull requests");
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
  }
});

test("bundled skill validation uses the root lockfile without network fallback", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const npmPackageJob = indentedBlock(workflow, "npm-package", 2);
  const rootPackage = JSON.parse(readFileSync("package.json", "utf8"));
  const nestedPackage = JSON.parse(readFileSync("npm/fallow/package.json", "utf8"));
  const lockfile = JSON.parse(readFileSync("package-lock.json", "utf8"));

  assert.equal(rootPackage.devDependencies["@tanstack/intent"], "0.4.0");
  assert.equal(
    nestedPackage.devDependencies?.["@tanstack/intent"],
    rootPackage.devDependencies["@tanstack/intent"],
  );
  assert.equal(lockfile.packages[""].devDependencies["@tanstack/intent"], "0.4.0");
  assert.equal(lockfile.packages["node_modules/@tanstack/intent"].version, "0.4.0");
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

test("Action PR comment smoke verifies the author that the broker outcome implies", () => {
  const workflow = readWorkflow(".github/workflows/test-action.yml");
  const job = indentedBlock(workflow, "test-comment", 2);

  assert.match(job, /permissions:\n\s+contents: read\n\s+id-token: write\n\s+pull-requests: write/);
  assert.match(
    job,
    /if: github\.event_name == 'pull_request' && github\.event\.pull_request\.head\.repo\.full_name == github\.repository/,
  );
  assert.match(job, /"\$COMMENT_COUNT" -ne 1/);
  assert.match(job, /COMMENT_AUTHOR=.*jq -r '\.\[0\]\.user\.login \/\/ empty'/);
  // A broker fallback posts as github-actions[bot], so the author check reads
  // the recorded outcome instead of a fixed author.
  assert.match(
    job,
    /COMMENT_AUTHOR="\$COMMENT_AUTHOR" node \.github\/scripts\/check-comment-author\.mjs/,
  );
  assert.doesNotMatch(job, /EXPECTED_COMMENT_AUTHOR/);
});

test("CI caches are scoped to the analyzed root", () => {
  const action = readWorkflow("action.yml");
  const cacheKey = action.match(/^\s+key: fallow-cache-.*$/m)?.[0];
  const restoreKey = action.match(/^\s+fallow-cache-\$\{\{ runner\.os \}\}[^\n]*$/m)?.[0];

  // `restore-keys` are literal string-prefix matches, so interpolating the
  // root raw does NOT partition roots: the restore prefix for
  // `root=packages/app` is a literal prefix of the real key for
  // `root=packages/app-admin`, and the first job restores the second's parse
  // cache. Only a fixed-width digest ends the root segment at a known length.
  assert.match(
    action,
    /root_digest=%s\\n' "\$\{digest:0:16\}" >> "\$GITHUB_OUTPUT"/,
    "Action must derive a fixed-width digest of inputs.root for the cache key",
  );
  assert.match(
    cacheKey ?? "",
    /\$\{\{ steps\.fallow-cache-key\.outputs\.root_digest \}\}/,
    "Action cache key must scope on the root digest",
  );
  assert.doesNotMatch(
    cacheKey ?? "",
    /\$\{\{ inputs\.root \}\}-/,
    "the raw root must not form a key segment: its restore prefix bleeds into a longer sibling root",
  );
  assert.match(
    restoreKey ?? "",
    /\$\{\{ steps\.fallow-cache-key\.outputs\.root_digest \}\}-$/,
    "Action restore-keys must end on the fixed-width root digest so a matrix over roots cannot restore a sibling cache",
  );

  // The GitLab twin needs no digest: GitLab rejects "/" inside a cache key, so
  // the root cannot be interpolated at all, and GitLab matches cache keys
  // exactly. Configuring `fallback_keys` would introduce prefix matching and
  // with it the same bleed, so its absence is part of the invariant.
  const gitlabCache = indentedBlock(readWorkflow("ci/gitlab-ci.yml"), "cache", 2);

  assert.match(gitlabCache, /key: "fallow-\$\{CI_COMMIT_REF_SLUG\}-\$\{CI_JOB_NAME_SLUG\}"/);
  assert.match(gitlabCache, /^\s+- \$\{FALLOW_ROOT\}\/\.fallow\/$/m);
  assert.doesNotMatch(
    gitlabCache,
    /fallback_keys/,
    "GitLab cache keys are exact matches; a fallback key list would reintroduce prefix bleed",
  );
});

// A predicate that names Windows or excludes Unix selects code that only
// compiles and runs on Windows. `any(unix, windows)` means "any supported host"
// and is not Windows-specific, so an `any(...)` group must not also name unix.
const windowsCfgPattern =
  /\bcfg(?:_attr)?!?\s*\(\s*(?:windows\b|target_(?:os|family)\s*=\s*"windows"|not\s*\(\s*unix\s*\)|all\s*\([^()]*(?:\bwindows\b|"windows"|not\s*\(\s*unix\s*\))|any\s*\((?![^()]*\bunix\b)[^()]*(?:\bwindows\b|"windows")|any\s*\([^()]*not\s*\(\s*unix\s*\))/;

const hasWindowsSpecificCode = (source) =>
  source
    .split(/\r?\n/)
    .some((line) => !line.trimStart().startsWith("//") && windowsCfgPattern.test(line));

const rustSourceFiles = (dir) =>
  readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return entry.name === "target" ? [] : rustSourceFiles(path);
    return entry.name.endsWith(".rs") ? [path.split("\\").join("/")] : [];
  });

test("Windows cfg detection covers the attribute and macro forms", () => {
  for (const line of [
    "#[cfg(windows)]",
    '#[cfg(target_os = "windows")]',
    "#[cfg(not(unix))]",
    "#[cfg(all(test, windows))]",
    'let normalized = if cfg!(any(target_os = "macos", target_os = "windows")) {',
    "if cfg!(windows) {",
    '#[cfg_attr(windows, ignore = "unix only")]',
  ]) {
    assert.ok(hasWindowsSpecificCode(line), `expected a Windows cfg match: ${line}`);
  }
  for (const line of [
    "#[cfg(unix)]",
    "#[cfg(not(windows))]",
    "#[cfg(any(unix, windows))]",
    "// #[cfg(windows)]",
  ]) {
    assert.ok(!hasWindowsSpecificCode(line), `expected no Windows cfg match: ${line}`);
  }
});

// Code behind a Windows cfg never compiles or runs on the Ubuntu jobs. A pull
// request that changes such a file must start a Windows job, or a Windows-only
// lint or failure reaches main unseen. Test files under `tests/` are in scope
// too: the Windows job builds and lints every test target of its packages.
// The `windows-type-aware` filter counts as coverage because its job runs the
// type-aware transport tests on Windows.
test("every Rust file with Windows-specific code starts a Windows CI job", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const windowsPaths = [
    ...listedPaths(indentedBlock(workflow, "windows-rust", 12)),
    ...listedPaths(indentedBlock(workflow, "windows-type-aware", 12)),
  ];
  const windowsFiles = rustSourceFiles("crates").filter((path) =>
    hasWindowsSpecificCode(readFileSync(path, "utf8")),
  );
  const uncovered = windowsFiles.filter((path) => !matchesListedPath(windowsPaths, path));
  const missing = windowsPaths.filter((path) => !path.endsWith("/**") && !existsSync(path));

  assert.deepEqual(missing, [], "remove or rename these stale Windows filter entries");

  assert.ok(windowsFiles.includes("crates/engine/src/write_guard.rs"));
  assert.deepEqual(
    uncovered,
    [],
    "add these files to the windows-rust path filter in .github/workflows/ci.yml",
  );
});

// The Windows job lints only the packages it names with `-p`. A crate with
// Windows-specific code that is not in that list starts the job but is never
// linted on Windows, so a Windows-only clippy failure reaches main unseen.
test("the Windows clippy step lints every crate with Windows-specific code", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const clippyLine = indentedBlock(workflow, "windows-rust", 2)
    .split("\n")
    .find((line) => line.includes("cargo clippy"));
  assert.ok(clippyLine, "the windows-rust job must run cargo clippy");
  const linted = new Set([...clippyLine.matchAll(/-p ([\w-]+)/g)].map((match) => match[1]));
  const packageOf = (path) => {
    const manifest = readFileSync(join("crates", path.split("/")[1], "Cargo.toml"), "utf8");
    return manifest.match(/^name\s*=\s*"([^"]+)"/m)[1];
  };
  const windowsPackages = new Set(
    rustSourceFiles("crates")
      .filter((path) => hasWindowsSpecificCode(readFileSync(path, "utf8")))
      .map(packageOf),
  );
  const unlinted = [...windowsPackages].filter((name) => !linted.has(name)).toSorted();

  assert.deepEqual(
    unlinted,
    [],
    "add these packages to the windows-rust clippy step in .github/workflows/ci.yml",
  );
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
  assert.match(checkJob, /runs-on:.*\|\| \x27ubuntu-26.04\x27/);
  assert.match(checkJob, /timeout-minutes: 30/);
  assert.doesNotMatch(checkJob, /matrix\.|windows-latest|macos-latest/);
  assert.match(vscodePackageTargetsJob, /runs-on: ubuntu-26.04/);
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
  // The write guard names the Windows null device, and its CLI test runs only
  // on Windows.
  assert.ok(windowsRustPaths.includes("crates/engine/src/write_guard.rs"));
  assert.ok(windowsRustPaths.includes("crates/cli/src/write_scope.rs"));
  assert.ok(windowsRustPaths.includes("crates/cli/tests/integration/exit_code_tests.rs"));
  assert.ok(windowsRustPaths.includes("crates/cli/src/signal/**"));
  assert.ok(windowsRustPaths.includes("crates/lsp/**"));
  // Release validation runs the drift harness on Windows, so a harness change
  // must run there on the pull request too.
  assert.ok(windowsRustPaths.includes("crates/cli/tests/drift/**"));
  assert.match(windowsRustJob, /^[ \t]+run: cargo build -p fallow-mcp$/m);
  assert.match(
    windowsRustJob,
    /^[ \t]+run: cargo test -p fallow-cli --test drift -- --include-ignored$/m,
  );
  // Path rendering lives across both crates (`Display`, `join`, `components`),
  // and a separator regression there is invisible until the weekly Release
  // Validation runs the full suite on Windows.
  assert.ok(windowsRustPaths.includes("crates/config/**"));
  assert.ok(windowsRustPaths.includes("crates/types/**"));
  // One nextest run selects the Windows-sensitive tests. Each filter group
  // must stay, or a platform regression is invisible until release validation.
  assert.match(windowsRustJob, /tool: cargo-nextest/);
  assert.match(windowsRustJob, /cargo nextest run --profile ci/);
  for (const group of [
    "package(fallow-engine) & (test(changed_files::tests) | test(churn::tests) | test(repo_refs::tests) | test(write_guard::tests))",
    "binary_id(fallow-cli::integration) & test(/^exit_code_tests::.*null_device/)",
    "package(fallow-core) & test(symlink)",
    "package(fallow-lsp) & test(windows_initialization_publishes_uri_safe_diagnostics)",
    "package(fallow-mcp) & test(completed_success_cleans_descendant_process_tree)",
    "package(fallow-process) & test(windows_job_object_terminates_descendants_without_taskkill_lookup)",
    "(package(fallow-config) | package(fallow-types)) & kind(lib)",
    "package(fallow-graph) & (test(package_source) | test(resolve_honors_) | test(static_dir_relative_path_safety))",
    "package(fallow-api) & (test(protocol_path_accepts_windows_verbatim_paths_within_root) | test(discovery_only_accepts_a_sibling_file))",
    "(binary_id(fallow-multicall::integration) & test(/^parity::/))",
  ]) {
    assert.ok(windowsRustJob.includes(group), `Windows nextest filter is missing ${group}`);
  }
  assert.match(windowsRustJob, /name: nextest-junit-windows-rust/);
  assert.match(
    windowsRustJob,
    /^[ \t]+run: cargo clippy -p fallow-cli -p fallow-core -p fallow-engine -p fallow-lsp -p fallow-mcp -p fallow-graph -p fallow-api -p fallow-multicall -p fallow-config -p fallow-process -p fallow-types --all-targets -- -D warnings$/m,
  );
  assert.match(windowsTypeAwareJob, /needs: changes/);
  assert.match(windowsTypeAwareJob, /if: "?needs\.changes\.outputs\.windows-type-aware == 'true'/);
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
  assert.match(zedJob, /runs-on: ubuntu-26.04/);
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

/** The workspace `rust-version`, with a `.0` patch added when Cargo.toml omits it. */
const workspaceMsrvToolchain = () => {
  const rustVersion = readFileSync("Cargo.toml", "utf8").match(/^rust-version = "([^"]+)"$/mu)?.[1];
  assert.ok(rustVersion, "Cargo.toml declares a workspace rust-version");
  return rustVersion.split(".").length === 2 ? `${rustVersion}.0` : rustVersion;
};

test("MSRV CI forces the Cargo.toml rust-version despite the repository toolchain override", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const msrvJob = indentedBlock(workflow, "msrv", 2);
  const toolchain = workspaceMsrvToolchain().replaceAll(".", "\\.");

  assert.match(msrvJob, new RegExp(`RUSTUP_TOOLCHAIN: ${toolchain}$`, "mu"));
  assert.match(msrvJob, new RegExp(`toolchain: '${toolchain}'$`, "mu"));
  assert.match(msrvJob, /run: cargo check --workspace/);
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
  assert.match(
    vscodeTargetJob,
    /name: Initialize pnpm store on Windows[\s\S]*if: runner\.os == 'Windows'[\s\S]*pnpm store path --silent[\s\S]*New-Item -ItemType Directory -Force -Path \$store/,
  );
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
  const similarCodeConformance = indentedBlock(workflow, "similar-code-conformance", 2);
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
  assert.match(build, /needs: \[release-context, pgo-profile\]/);
  assert.match(validate, /needs: release-context/);
  assert.match(similarCodeConformance, /needs: build/);
  assert.match(similarCodeConformance, /permissions:\n\s+contents: read/);
  assert.doesNotMatch(similarCodeConformance, /id-token: write|contents: write/);
  assert.match(similarCodeConformance, /fallow-similar-code-linux-x64-gnu/);
  assert.match(similarCodeConformance, /verify-binary\.mjs/);
  assert.match(similarCodeConformance, /release_binary_max_bytes/);
  assert.match(similarCodeConformance, /check-similar-code-sidecar-audit\.mjs/);
  assert.match(similarCodeConformance, /cargo audit[\s\S]*--ignore RUSTSEC-2024-0436/u);
  assert.match(
    similarCodeConformance,
    /cargo deny[\s\S]*tools\/similar-code-sidecar\/Cargo\.toml[\s\S]*check licenses/u,
  );
  assert.match(similarCodeConformance, /semantic-clone-candle-conformance\.mjs/);
  assert.match(gate, /needs: \[build, validate, similar-code-conformance\]/);
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
    assert.match(
      job,
      /needs: \[vscode-prep, vscode-host-smoke, release-assets, npm-root-approved\]/,
      registry,
    );
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
    assert.match(job, /delays=\(20 40 60 90 120\)/u, registry);
    assert.match(
      job,
      /while \[ "\$\{#pending\[@\]\}" -ne 0 \]; do[\s\S]*--skip-duplicate; then\n\s+remaining\+=\("\$file"\)/u,
      registry,
    );
    assert.match(
      job,
      /if \[ "\$pass" -gt "\$\{#delays\[@\]\}" \]; then\n\s+for file in "\$\{remaining\[@\]\}"; do\n\s+echo "::error::/u,
      registry,
    );
    assert.match(
      job,
      /echo "::warning::[^\n]*\$\{remaining\[\*\]\}[^\n]*"\n\s+sleep "\$delay"/u,
      registry,
    );
    assert.match(job, /timeout-minutes: 45/u, registry);
  }

  assert.match(
    vscodePublicVerify,
    /needs: \[vscode-prep, vscode-publish-marketplace, vscode-publish-open-vsx\]/,
  );
  assert.match(vscodePublicVerify, /persist-credentials: false/);
  assert.match(vscodePublicVerify, /timeout-minutes: 45/);
  assert.match(vscodePublicVerify, /node scripts\/vscode-public-verify\.mjs --artifact-dir/);
  assert.doesNotMatch(vscodePublicVerify, /secrets\.|_PAT|npm install|pnpm install/u);
  assert.match(
    releaseReady,
    /needs: \[publish-crates, npm-publish, npm-root-approved, vscode-public-verify, release-assets\]/,
  );
  assert.match(releaseReady, /permissions:\n\s+contents: read/);
  assert.match(releaseReady, /Release tag .* appeared before the release workflow completed/u);
});

test("release stages the fallow npm root for maintainer approval", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const npmPublish = indentedBlock(workflow, "npm-publish", 2);
  const security = readFileSync("docs/development/release-security.md", "utf8");
  const stageCalls = npmPublish.match(/^\s+stage_output=\$\(.*npm stage publish /gmu) ?? [];
  const stagedBranch = npmPublish.indexOf('if is_staged_name "$name"; then');
  const directPublish = npmPublish.indexOf('if ! npm publish "$file"');

  assert.match(npmPublish, /^\s+STAGED_NAMES=\("fallow"\)$/mu);
  assert.equal(stageCalls.length, 1, "npm-publish must make exactly one stage call");
  assert.match(
    npmPublish,
    /NODE_AUTH_TOKEN="" npm stage publish "\$file" --access public --provenance --ignore-scripts/u,
  );
  assert.notEqual(stagedBranch, -1, "staged names must branch before the direct publish");
  assert.ok(stagedBranch < directPublish, "a staged name must never reach npm publish");
  assert.match(
    npmPublish.slice(stagedBranch, directPublish),
    /index=\$\(\(index \+ 1\)\)\n\s+continue\n\s+fi\n\s*$/u,
    "the staged branch must end by skipping the direct publish",
  );
  assert.match(npmPublish, /grep -q '\^npm error code E409\$'/u);
  assert.match(npmPublish, /npm install -g --ignore-scripts npm@11\.19\.0/u);
  assert.match(security, /Stage the `fallow` npm root, never publish it from the workflow/u);
  assert.match(security, /exactly one `npm stage publish` call/u);
});

test("release publishes no VSIX before the approved fallow root is public", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const npmPublish = indentedBlock(workflow, "npm-publish", 2);
  const gate = indentedBlock(workflow, "npm-root-approved", 2);
  const marketplace = indentedBlock(workflow, "vscode-publish-marketplace", 2);
  const openVsx = indentedBlock(workflow, "vscode-publish-open-vsx", 2);
  const security = readFileSync("docs/development/release-security.md", "utf8");
  const procedure = readFileSync("docs/development/release-procedure.md", "utf8");

  assert.match(npmPublish, /^\s+id: publish$/mu);
  assert.match(npmPublish, /fallow_sha256: \$\{\{ steps\.publish\.outputs\.fallow_sha256 \}\}/u);
  assert.match(npmPublish, /echo "fallow_sha256=\$digest" >> "\$GITHUB_OUTPUT"/u);
  assert.match(gate, /^\s+needs: npm-publish$/mu);
  assert.match(gate, /permissions:\n\s+contents: read/u);
  assert.doesNotMatch(gate, /^\s+environment:|secrets\.|id-token: write|actions\/checkout/mu);
  assert.match(gate, /EXPECTED_SHA256: \$\{\{ needs\.npm-publish\.outputs\.fallow_sha256 \}\}/u);
  assert.match(gate, /curl -fsS[^\n]*"\$\{REGISTRY\}\/fallow\/\$\{VERSION\}"/u);
  assert.match(gate, /sha256sum fallow-public\.tgz/u);
  assert.match(gate, /"\$actual_sha256" != "\$EXPECTED_SHA256"/u);
  assert.match(gate, /rerun the failed jobs of this run/u);
  assert.match(gate, /^\s+timeout-minutes: 360$/mu);
  assert.match(gate, /WAIT_MINUTES: '3[0-5][0-9]'/u);
  for (const publisher of [marketplace, openVsx]) {
    assert.match(
      publisher,
      /needs: \[vscode-prep, vscode-host-smoke, release-assets, npm-root-approved\]/u,
    );
  }
  assert.match(security, /Publish no VSIX before the approved `fallow` root is public/u);
  assert.match(procedure, /Wait for the approved fallow root/u);
  assert.match(procedure, /Do this right after\s+`release-ready` without other work in between/u);
});

test("release credential jobs run in the main-only release environment", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const procedure = readFileSync("docs/development/release-procedure.md", "utf8");
  const security = readFileSync("docs/development/release-security.md", "utf8");
  const credentialJobs = [
    "build",
    "npm-publish",
    "publish-crates",
    "vscode-publish-marketplace",
    "vscode-publish-open-vsx",
  ];
  const jobNames = Array.from(
    indentedBlock(workflow, "jobs", 0).matchAll(/^ {2}([a-z][a-z0-9-]*):$/gmu),
    (match) => match[1],
  );
  const environmentJobs = [];

  for (const name of jobNames) {
    const job = indentedBlock(workflow, name, 2);
    const inEnvironment = /^ {4}environment: release$/mu.test(job);
    const holdsCredentials =
      /\$\{\{\s*secrets\.(?!GITHUB_TOKEN\b)/u.test(job) || /^\s+id-token: write$/mu.test(job);

    if (inEnvironment) {
      environmentJobs.push(name);
    }
    assert.ok(
      !holdsCredentials || inEnvironment,
      `${name} holds publication credentials outside the release environment`,
    );
    assert.doesNotMatch(job, /^ {4}environment:(?! release$)/mu);
  }

  assert.deepEqual(environmentJobs.toSorted(), credentialJobs);
  assert.match(procedure, /environments\/release\/deployment-branch-policies/u);
  assert.match(procedure, /exists at both levels; delete the repository copy/u);
  assert.match(procedure, /unprotected by the release environment/u);
  assert.match(security, /Its deployment branch policy admits `main` only/u);
});

test("release keeps the version tag last and requires curated public notes", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const context = indentedBlock(workflow, "release-context", 2);
  const releaseAssets = indentedBlock(workflow, "release-assets", 2);
  const procedure = readFileSync("docs/development/release-procedure.md", "utf8");
  const downloadStep = releaseAssets.indexOf("- name: Download all artifacts");
  const absentTagStep = releaseAssets.indexOf("- name: Reconfirm release tag is absent");
  const assembleStep = releaseAssets.indexOf("- name: Assemble release asset bundle");
  const uploadStep = releaseAssets.indexOf("- name: Upload release asset bundle");
  const workflowDispatch = procedure.indexOf("gh workflow run release.yml");
  const downloadBundle = procedure.indexOf("--name release-assets");
  const stageDigestCheck = procedure.indexOf('test "$BUILT" = "$STAGED"');
  const stageApprove = procedure.indexOf('npm stage approve "$STAGE_ID"');
  const signedTag = procedure.indexOf('git tag -s "$TAG"');
  const createRelease = procedure.indexOf('gh release create "$TAG"');

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
  assert.match(procedure, /immutable-releases/u);
  assert.match(procedure, /Release immutability is not enabled/u);
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

  assert.match(
    procedure,
    /Draft curated public GitHub release notes before starting the publication/u,
  );
  assert.match(procedure, /exact full-changelog comparison URL/u);
  assert.match(procedure, /non-empty body/u);
  assert.match(procedure, /release title must be/u);
  assert.match(procedure, /release title or notes contain an em-dash/u);
  assert.match(procedure, /name no competing or upstream third-party project/u);
  // The curated notes are drafted from the changelog section, which is the one
  // half of the metadata the tag-last workflow can still read at dispatch.
  assert.match(context, /node scripts\/verify-release-metadata\.mjs --tag "\$TAG_NAME"/u);
  assert.match(procedure, /scripts\/verify-release-metadata\.mjs/u);
  assert.match(procedure, /--name release-assets/u);
  assert.match(procedure, /--verify-tag/u);
  assert.match(procedure, /--notes-file "\$NOTES_FILE"/u);
  assert.notEqual(workflowDispatch, -1, "procedure must dispatch the release workflow");
  assert.notEqual(downloadBundle, -1, "procedure must download the exact run asset bundle");
  assert.notEqual(signedTag, -1, "procedure must create a signed release tag");
  assert.notEqual(createRelease, -1, "procedure must create the immutable release");
  assert.ok(workflowDispatch < downloadBundle, "workflow must complete before asset download");
  assert.ok(downloadBundle < signedTag, "asset bundle must exist before tag creation");
  assert.notEqual(
    stageDigestCheck,
    -1,
    "procedure must compare the staged root with the run artifact",
  );
  assert.notEqual(stageApprove, -1, "procedure must approve the staged fallow root");
  assert.ok(workflowDispatch < stageDigestCheck, "workflow must complete before the stage check");
  assert.ok(stageDigestCheck < stageApprove, "staged bytes must be verified before approval");
  assert.ok(stageApprove < signedTag, "the staged root must be approved before tag creation");
  assert.ok(signedTag < createRelease, "signed tag must exist before release creation");
});

test("publication re-checks the release metadata the tag-last workflow cannot read", () => {
  const workflow = readWorkflow(".github/workflows/release-published.yml");
  const job = indentedBlock(workflow, "release-metadata", 2);
  const procedure = readFileSync("docs/development/release-procedure.md", "utf8");

  assert.match(workflow, /^on:\n {2}release:\n {4}types: \[published\]$/mu);
  assert.match(workflow, /^permissions: \{\}$/mu);
  assert.match(job, /permissions:\n\s+contents: read/u);
  assert.match(job, /persist-credentials: false/u);
  assert.match(job, /--json name,body,isDraft,isPrerelease/u);
  assert.match(job, /node scripts\/verify-release-metadata\.mjs/u);
  assert.match(job, /--release-json release\.json/u);

  // The tag name comes from the event, so it reaches the shell as an
  // environment variable and never as a template expansion inside `run:`.
  assert.match(job, /TAG_NAME: \$\{\{ github\.event\.release\.tag_name \}\}/u);
  const runBlocks = job
    .split(/^\s+run: \|?$/mu)
    .slice(1)
    .join("\n");
  assert.doesNotMatch(runBlocks, /\$\{\{ github\.event/u);

  // Checking out an event-controlled ref is what makes a release-triggered
  // workflow exploitable; the default branch already carries the changelog.
  assert.doesNotMatch(job, /^\s+ref:/mu);

  assert.match(procedure, /release-published\.yml/u);
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
  assert.match(vscodeJob, /version: 11\.25\.0/);
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
  assert.match(packageJson, /"packageManager": "pnpm@11\.25\.0"/);
  assert.match(harness, /version: "1\.96\.0"/);
});

test("coverage runs with read-only permissions on every push to main", () => {
  const workflow = readWorkflow(".github/workflows/coverage.yml");
  const coverageJob = indentedBlock(workflow, "coverage", 2);
  const pushTrigger = indentedBlock(workflow, "push", 2);

  // Coverage moved off pull requests to keep the job count low on the free
  // runner plan. The release gate needs a coverage run on each release
  // commit, so the push trigger must not have a path filter.
  assert.doesNotMatch(workflow, /^  pull_request:/m, "coverage runs on main, not on pull requests");
  assert.match(workflow, /^  workflow_dispatch:$/m);
  assert.match(pushTrigger, /branches: \[main\]/);
  assert.doesNotMatch(pushTrigger, /paths:/, "every push to main must get a coverage run");
  assert.match(workflow, /startsWith\(github\.event\.head_commit\.message, 'chore: release v'\)/);
  assert.match(coverageJob, /^    name: Coverage$/m);
  assert.match(coverageJob, /permissions:\n\s+contents: read/);
  assert.match(coverageJob, /persist-credentials: false/);
  assert.match(coverageJob, /name: Enforce coverage floor/);
  assert.doesNotMatch(coverageJob, /coverage_policy/);
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
  assert.doesNotMatch(publishJob, /store-benchmark|gh-pages/);
  assert.match(publishJob, /name: Update coverage badge/);
  assert.doesNotMatch(publishJob, /\b(?:cargo|npm|pnpm)\b/);
});

test("coverage producer conformance rides the check job instead of a job of its own", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const checkJob = indentedBlock(workflow, "check", 2);
  const rustPaths = listedPaths(indentedBlock(workflow, "rust", 12));

  assert.match(
    checkJob,
    /name: Run coverage producer conformance\n(?:\s+#.*\n)*\s+run: npm run check:coverage-producers/,
    "deleting the census step must fail this test rather than pass silently",
  );
  assert.ok(
    checkJob.indexOf("name: Run tests") < checkJob.indexOf("name: Run coverage producer"),
    "the census needs the binary cargo test --bins already produced",
  );
  assert.ok(
    matchesListedPath(rustPaths, "tests/coverage-producer-corpus/manifest.json"),
    "the corpus must already be inside the filter that gates the check job",
  );
  assert.ok(matchesListedPath(rustPaths, "crates/engine/src/health/scoring.rs"));
});

test("the pinned coverage producers are covered by dependabot", () => {
  const config = readWorkflow(".github/dependabot.yml");

  assert.match(
    config,
    /package-ecosystem: npm\n\s+directory: \/tests\/coverage-producer-corpus\/producers/,
    "no npm directory is covered automatically, so the pin would never move",
  );
});

test("push runs of the release commit are never cancelled", async () => {
  const { REQUIRED_WORKFLOWS } = await import("./verify-release-ci.mjs");

  // The release gate needs a finished run of each required workflow on the
  // release commit. A workflow that cancels older push runs must give the
  // release commit a concurrency group of its own.
  for (const { file } of REQUIRED_WORKFLOWS) {
    const workflow = readWorkflow(`.github/workflows/${file}`);
    const cancel = workflow.match(/^ {2}cancel-in-progress: (.+)$/m)?.[1];
    if (cancel === undefined || cancel === "false") continue;
    const concurrency = indentedBlock(workflow, "concurrency", 0);
    if (cancel === "${{ github.event_name == 'pull_request' }}") {
      // Push runs never cancel. A group per commit also keeps a queued run
      // from being dropped when a newer run of the group queues.
      assert.match(concurrency, /github\.(sha|run_id)/, `${file} needs a group per push run`);
      continue;
    }
    for (const key of ["group", "cancel-in-progress"]) {
      assert.match(
        concurrency.match(new RegExp(`^ {2}${key}: (.+)$`, "m"))?.[1] ?? "",
        /!startsWith\(github\.event\.head_commit\.message, 'chore: release v'\)/,
        `${file} ${key} must exempt the release commit`,
      );
    }
  }
});

test("the release checks the release commit subject that concurrency exempts", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const context = indentedBlock(workflow, "release-context", 2);

  assert.match(context, /git log -1 --format=%s/);
  assert.match(context, /"chore: release \$\{TAG_NAME\}"/);
  assert.ok(
    context.indexOf("chore: release ${TAG_NAME}") <
      context.indexOf("scripts/verify-release-ci.mjs"),
    "the subject check must run before the CI gate waits",
  );
});

test("mise.toml follows the tool versions that CI pins", () => {
  const mise = readFileSync("mise.toml", "utf8");
  const workflowDir = ".github/workflows";
  const workflows = readdirSync(workflowDir)
    .filter((file) => file.endsWith(".yml"))
    .map((file) => [file, readWorkflow(join(workflowDir, file))]);
  const miseVersion = (tool) =>
    mise.match(new RegExp(`^"?(?:[^"=\\s]*[:/])?${tool}"? *= *"([^"]+)"`, "mu"))?.[1];

  const nodeVersion = miseVersion("node");
  assert.ok(nodeVersion, "mise.toml must pin node");
  const ciNodeVersions = Array.from(
    readWorkflow(join(workflowDir, "ci.yml")).matchAll(/node-version: ['"]?([^'"\s]+)/gu),
    (match) => match[1],
  );
  assert.ok(ciNodeVersions.length > 0, "ci.yml must set up node");
  for (const version of ciNodeVersions) {
    assert.equal(nodeVersion, version, "mise.toml node must match ci.yml setup-node");
  }

  // A tool that CI installs at an exact version must have that version in mise.toml.
  for (const [file, workflow] of workflows) {
    for (const [, tool, version] of workflow.matchAll(/^\s+tool: ([\w-]+)@([\w.-]+)$/gmu)) {
      const pinned = miseVersion(tool);
      if (pinned === undefined) continue;
      assert.equal(pinned, version, `${file} pins ${tool}@${version}; mise.toml has ${pinned}`);
    }
  }
});

const PGO_CONFIG_FLAG = "--config target/pgo-profile/pgo.toml";
const PGO_RUST_BINARY_PACKAGES = ["fallow-cli", "fallow-lsp", "fallow-mcp", "fallow-multicall"];
// A profile matches only a build for the same target on the same runner and
// container, so each PGO target has a training leg that mirrors its build leg.
const PGO_LEGS = [
  {
    target: "x86_64-unknown-linux-gnu",
    os: "ubuntu-26.04",
    container: /container: rust:1\.97\.1-bullseye@sha256:[0-9a-f]{64}/u,
  },
  { target: "aarch64-apple-darwin", os: "macos-latest", container: null },
];

const cargoRunLines = (job) =>
  Array.from(job.matchAll(/^\s+run: (.*\bcargo (?:build|zigbuild)\b.*)$/gmu), (match) => match[1]);

const workflowSteps = (job) => job.split(/^\s{6}- (?=name: |uses: )/mu).slice(1);

const matrixEntries = (job) =>
  job
    .split(/^\s{4}steps:/mu)[0]
    .split(/^\s{10}- /mu)
    .slice(1);

const matrixEntry = (job, target) =>
  matrixEntries(job).find((entry) => new RegExp(`target: ${target}\\n`, "u").test(entry));

test("release builds every Rust binary with the PGO config and nothing else", () => {
  const build = indentedBlock(readWorkflow(".github/workflows/release.yml"), "build", 2);
  const runs = cargoRunLines(build);
  const binaryRuns = runs.filter((run) => /-p fallow-(?:cli|lsp|mcp|multicall)\b/u.test(run));
  const otherRuns = runs.filter((run) => !binaryRuns.includes(run));
  const napiSteps = workflowSteps(build).filter((step) => /napi build/u.test(step));
  const combined = binaryRuns.filter((run) => run.match(/-p fallow-/gu).length > 1);

  // One combined build per leg, plus four separate builds on aarch64-musl.
  assert.equal(binaryRuns.length, 5);
  assert.equal(combined.length, 1, "one cargo invocation builds the four binaries");
  assert.deepEqual(
    Array.from(combined[0].matchAll(/-p (fallow-[a-z]+)/gu), (match) => match[1]),
    PGO_RUST_BINARY_PACKAGES,
  );
  assert.deepEqual(
    [...new Set(binaryRuns.flatMap((run) => run.match(/fallow-[a-z]+/gu)))].toSorted(),
    PGO_RUST_BINARY_PACKAGES,
  );
  for (const run of binaryRuns) {
    assert.ok(run.includes(PGO_CONFIG_FLAG), `${run} must read the PGO config`);
  }
  assert.equal(otherRuns.length, 2, "the similar-code provider builds twice");
  for (const run of otherRuns) {
    assert.match(run, /tools\/similar-code-sidecar\/Cargo\.toml/u);
    assert.doesNotMatch(run, /pgo|profile-use/u, "the similar-code provider must not get PGO");
  }
  assert.equal(napiSteps.length, 1);
  assert.doesNotMatch(napiSteps[0], /pgo|profile-use/u, "the NAPI addon must not get PGO");
});

test("release trains one PGO profile per PGO target and can build without it", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const dispatch = indentedBlock(workflow, "workflow_dispatch", 2);
  const profileJob = indentedBlock(workflow, "pgo-profile", 2);
  const build = indentedBlock(workflow, "build", 2);
  const steps = workflowSteps(build);
  const resolve = steps.find((step) => step.startsWith("name: Resolve PGO"));
  const match = steps.find((step) => step.startsWith("name: Check that the PGO profile matches"));
  const combined = steps.find((step) => step.startsWith("name: Build Rust release binaries"));

  assert.match(dispatch, /pgo:\n(?:\s{8}.*\n)*?\s{8}default: true\n\s{8}type: boolean/u);
  assert.match(profileJob, /needs: release-context/u);
  assert.match(profileJob, /PGO_INPUT: \$\{\{ inputs\.pgo \}\}/u);
  assert.match(profileJob, /components: llvm-tools/u);
  assert.match(profileJob, /rustflags = \['-Cprofile-generate=/u);
  assert.match(profileJob, /-p fallow-multicall --config target\/pgo-generate\.toml/u);
  assert.match(profileJob, /scripts\/pgo-train\.sh/u);
  assert.match(profileJob, /download-fixtures\.mjs --only preact,fastify,zod,vue-core,svelte/u);
  assert.match(
    profileJob,
    /key: pgo-train-fixtures-[^\n]*runner\.os[^\n]*hashFiles\('benchmarks\/download-fixtures\.mjs'\)/u,
  );
  assert.doesNotMatch(profileJob, /^\s+id-token: write$|secrets\./mu);
  // Only the profile leaves the job. A `fallow-` artifact would become a release asset.
  const uploads = Array.from(profileJob.matchAll(/^\s+name: (.+)\n\s+path: (.+)$/gmu));
  assert.deepEqual(
    uploads.map((upload) => [upload[1], upload[2]]),
    [["pgo-profile-${{ matrix.target }}", "pgo-profile/fallow.profdata"]],
  );

  const trainLegs = matrixEntries(profileJob);
  const pgoBuildLegs = matrixEntries(build).filter((entry) => /pgo_profile: true/u.test(entry));
  assert.equal(trainLegs.length, PGO_LEGS.length);
  assert.equal(pgoBuildLegs.length, PGO_LEGS.length, "only the trained targets get a profile");
  for (const leg of PGO_LEGS) {
    const train = matrixEntry(profileJob, leg.target);
    const buildLeg = matrixEntry(build, leg.target);
    assert.ok(train, `${leg.target} needs a training leg`);
    assert.match(buildLeg, /pgo_profile: true/u);
    for (const entry of [train, buildLeg]) {
      assert.match(entry, new RegExp(`os: ${leg.os}\\n`, "u"));
      if (leg.container) {
        assert.match(entry, leg.container);
      } else {
        assert.doesNotMatch(entry, /container:/u);
      }
    }
    assert.equal(
      train.match(/container: (.+)/u)?.[1],
      buildLeg.match(/container: (.+)/u)?.[1],
      `${leg.target} must train in the container of its build leg`,
    );
  }

  assert.match(
    build,
    /name: pgo-profile-\$\{\{ matrix\.target \}\}\n\s+path: target\/pgo-profile/u,
  );
  assert.match(
    build,
    /if: needs\.pgo-profile\.outputs\.enabled == 'true' && matrix\.pgo_profile\n/u,
  );
  assert.ok(resolve, "the build job must resolve the PGO flags");
  assert.match(resolve, /PGO_ENABLED: \$\{\{ needs\.pgo-profile\.outputs\.enabled \}\}/u);
  assert.match(resolve, /PGO_PROFILE_LEG: \$\{\{ matrix\.pgo_profile && 'true' \|\| 'false' \}\}/u);
  assert.match(
    resolve,
    /\[target\.%s\]\\nrustflags = \['-Cprofile-use=%s', '-Cllvm-args=-pgo-warn-missing-function'\]/u,
  );
  assert.match(resolve, /PGO is off for this release/u);
  assert.match(resolve, /No PGO profile for/u);
  assert.ok(combined, "one step builds the four Rust binaries");
  assert.match(combined, /shell: bash/u);
  assert.match(combined, /\| tee target\/pgo-profile\/build\.log/u);
  assert.ok(match, "the build job must check that the profile matches the build");
  assert.match(match, /if: needs\.pgo-profile\.outputs\.enabled == 'true' && matrix\.pgo_profile/u);
  assert.match(match, /pgo-profile-match\.mjs/u);
  assert.match(match, /--log target\/pgo-profile\/build\.log/u);
  assert.match(build, /components: \$\{\{ matrix\.pgo_profile && 'llvm-tools' \|\| '' \}\}/u);
  assert.match(build, /name: Verify release binaries carry no PGO instrumentation/u);
  assert.match(build, /grep -qa LLVM_PROFILE_FILE/u);
});

test("no PGO workflow sets a global RUSTFLAGS that drops the Windows stack flags", () => {
  const config = readFileSync(".cargo/config.toml", "utf8");
  for (const target of ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"]) {
    assert.match(
      config,
      new RegExp(
        `\\[target\\.${target}\\]\\nrustflags = \\["-C", "link-arg=/STACK:16777216"\\]`,
        "u",
      ),
    );
  }
  for (const file of ["release.yml", "pgo-validate.yml"]) {
    const workflow = readWorkflow(join(".github/workflows", file));
    // RUSTFLAGS and CARGO_ENCODED_RUSTFLAGS replace the target rustflags from
    // .cargo/config.toml. Cargo ignores CARGO_BUILD_RUSTFLAGS when a target
    // has rustflags, so the PGO flag would be lost on Windows.
    assert.doesNotMatch(
      workflow,
      // The lookbehind allows CARGO_TARGET_<TRIPLE>_RUSTFLAGS. The optional
      // space catches the pwsh form `$env:RUSTFLAGS = "..."`.
      /(?<![A-Za-z0-9_])(?:RUSTFLAGS|CARGO_ENCODED_RUSTFLAGS|CARGO_BUILD_RUSTFLAGS)\s*[:=]/mu,
      `${file} must not set a global RUSTFLAGS`,
    );
  }
});

test("pgo-validate gates PGO on the held-out fixtures for the PGO paths", () => {
  const workflow = readWorkflow(".github/workflows/pgo-validate.yml");
  const release = readWorkflow(".github/workflows/release.yml");
  const pullRequest = indentedBlock(workflow, "pull_request", 2);
  const train = indentedBlock(workflow, "train", 2);
  const compare = indentedBlock(workflow, "compare", 2);
  const script = readFileSync("scripts/pgo-train.sh", "utf8");
  const trainFixtures = script.match(/^readonly TRAIN_FIXTURES=\(([^)]+)\)$/mu)?.[1].split(" ");

  assert.deepEqual(listedPaths(pullRequest), [
    "scripts/pgo-train.sh",
    ".github/scripts/pgo-compare.mjs",
    ".github/scripts/pgo-profile-match.mjs",
    "benchmarks/download-fixtures.mjs",
    ".github/actions/setup-rust/**",
    ".cargo/config.toml",
    ".github/workflows/pgo-validate.yml",
    ".github/workflows/release.yml",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
  ]);
  assert.match(workflow, /^ {2}workflow_dispatch:$/mu);
  assert.match(workflow, /^permissions: \{\}$/mu);
  assert.match(train, /scripts\/pgo-train\.sh/u);
  assert.match(train, /components: llvm-tools/u);
  assert.deepEqual(trainFixtures, ["preact", "fastify", "zod", "vue-core", "svelte"]);
  for (const heldOut of ["query", "vite", "astro"]) {
    assert.ok(!trainFixtures.includes(heldOut), `${heldOut} is a held-out fixture`);
  }
  // The same PGO targets, runners and container as release.yml.
  const releaseProfile = indentedBlock(release, "pgo-profile", 2);
  for (const leg of PGO_LEGS) {
    const validateTrain = matrixEntry(train, leg.target);
    assert.ok(validateTrain, `${leg.target} needs a training leg`);
    assert.match(validateTrain, new RegExp(`os: ${leg.os}\\n`, "u"));
    assert.equal(
      validateTrain.match(/container: (.+)/u)?.[1],
      matrixEntry(releaseProfile, leg.target).match(/container: (.+)/u)?.[1],
    );
    assert.ok(matrixEntry(compare, leg.target), `${leg.target} needs a compare leg`);
  }
  assert.equal(matrixEntries(train).length, PGO_LEGS.length);
  assert.equal(matrixEntries(compare).length, PGO_LEGS.length);
  assert.match(compare, /--only query,vite,astro/u);
  assert.match(compare, /--projects 'query,vite,astro'/u);
  assert.match(compare, /--min-gain 0\.05/u);
  assert.match(compare, /grep -qx 'Gate: pass'/u, "a silent exit 0 must not pass the gate");
  assert.match(compare, /pgo-profile-match\.mjs/u);
  assert.match(compare, /-Cllvm-args=-pgo-warn-missing-function/u);
  assert.match(
    compare,
    /- os: ubuntu-26.04\n\s+target: x86_64-unknown-linux-gnu\n(?:\s+\w+: .*\n)*?\s+gate: true/u,
  );
  assert.equal((compare.match(/gate: true/gu) ?? []).length, 1, "only Linux x64 gates wall time");
  assert.ok(compare.includes(PGO_CONFIG_FLAG), "the PGO build must use the release mechanism");
});

test("Miri falls back to GitHub when runner selection fails or a job is rerun", async () => {
  const { runInNewContext } = await import("node:vm");
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const miri = indentedBlock(workflow, "miri", 2);
  const condition = miri.match(/^    if: (.+)$/m)?.[1];
  assert.match(condition, /always\(\)/, "selector failures must not skip Miri");
  assert.match(condition, /!cancelled\(\)/);
  const runner = miri.match(/^    runs-on: \$\{\{ (.+) \}\}$/m)?.[1];
  assert.ok(runner, "Miri must select a runner with GitHub fallback");
  const context = {
    github: { run_attempt: 1, event_name: "push" },
    needs: {
      changes: { outputs: { miri: "true" } },
      "miri-runner": { result: "success", outputs: { runner: "blacksmith-4vcpu-ubuntu-2404" } },
    },
    always: () => true,
    cancelled: () => false,
  };
  assert.equal(runInNewContext(condition, context), true);
  assert.equal(runInNewContext(runner, context), "blacksmith-4vcpu-ubuntu-2404");
  for (const result of ["failure", "skipped", "cancelled"]) {
    context.needs["miri-runner"].result = result;
    assert.equal(runInNewContext(condition, context), true);
    assert.equal(runInNewContext(runner, context), "ubuntu-26.04", result);
  }
  context.needs["miri-runner"].result = "success";
  context.github.run_attempt = 2;
  assert.equal(
    runInNewContext(runner, context),
    "ubuntu-26.04",
    "rerun-failed-jobs may reuse old selector output",
  );
  context.github.run_attempt = 1;
  context.needs["miri-runner"].outputs = {};
  assert.equal(runInNewContext(runner, context), "ubuntu-26.04", "missing output falls back");
  context.needs["miri-runner"].outputs.runner = "unexpected-runner";
  assert.equal(runInNewContext(runner, context), "ubuntu-26.04", "unknown labels fail closed");
  context.cancelled = () => true;
  assert.equal(runInNewContext(condition, context), false);
  context.cancelled = () => false;
  context.github.event_name = "pull_request";
  context.needs.changes.outputs.miri = "false";
  assert.equal(
    runInNewContext(condition, context),
    false,
    "original Miri path filter remains effective",
  );
});

test("Miri allocation uses a trusted optional helper and routing edits trigger Miri", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  assert.match(workflow, /^  miri-runner:$/m, "CI must run the optional allocation selector");
  const selector = indentedBlock(workflow, "miri-runner", 2);
  assert.match(selector, /runs-on: ubuntu-26.04/);
  assert.match(selector, /permissions:\n      contents: read\n/);
  assert.doesNotMatch(selector, /secrets\.|id-token:|actions: write|pull-requests:/);
  assert.match(selector, /ref: refs\/heads\/main/);
  assert.match(selector, /persist-credentials: false/);
  assert.match(selector, /github\.repository == 'fallow-rs\/fallow'/);
  assert.match(
    selector,
    /github\.event\.pull_request\.head\.repo\.full_name == github\.repository/,
  );
  assert.match(
    selector,
    /BLACKSMITH_MIRI_ALLOCATION: \$\{\{ vars\.BLACKSMITH_MIRI_ALLOCATION \}\}/,
  );
  assert.match(selector, /continue-on-error: true/);
  assert.match(
    selector,
    /if \[ ! -f scripts\/select-miri-runner\.mjs \]; then[\s\S]*runner=ubuntu-26.04[\s\S]*else[\s\S]*node scripts\/select-miri-runner\.mjs/,
  );
  const miri = indentedBlock(workflow, "miri", 2);
  assert.doesNotMatch(miri, /continue-on-error/);
  const changes = indentedBlock(workflow, "changes", 2);
  const paths = listedPaths(indentedBlock(changes, "miri", 12));
  const rustPaths = listedPaths(indentedBlock(changes, "rust", 12));
  for (const path of [
    ".github/workflows/ci.yml",
    "scripts/select-miri-runner.mjs",
    "scripts/select-miri-runner.test.mjs",
    "scripts/workflow-policy.test.mjs",
  ]) {
    assert.ok(paths.includes(path), `Miri filter is missing ${path}`);
    assert.ok(rustPaths.includes(path), `Rust filter is missing ${path}`);
  }
});

test("an unset Miri allocation skips selector startup without skipping Miri", async () => {
  const { runInNewContext } = await import("node:vm");
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const selector = indentedBlock(workflow, "miri-runner", 2);
  const condition = selector.match(/    if: >-\n([\s\S]+?)\n    runs-on:/)?.[1].trim();
  assert.ok(condition, "selector must declare its eligibility condition");
  const context = {
    vars: { BLACKSMITH_MIRI_ALLOCATION: "" },
    github: {
      repository: "fallow-rs/fallow",
      event_name: "push",
      ref: "refs/heads/main",
      run_attempt: 1,
      actor: "maintainer",
    },
    needs: {
      changes: { outputs: { miri: "true" } },
      "miri-runner": { result: "skipped", outputs: {} },
    },
    always: () => true,
    cancelled: () => false,
  };
  assert.equal(
    runInNewContext(condition, context),
    false,
    "default CI must not start a selector runner",
  );
  context.vars.BLACKSMITH_MIRI_ALLOCATION = '{"month":"2026-09","firstRunNumber":100,"slots":2}';
  assert.equal(runInNewContext(condition, context), true, "an allocation must reach the selector");
  const miri = indentedBlock(workflow, "miri", 2);
  assert.equal(runInNewContext(miri.match(/^    if: (.+)$/m)[1], context), true);
  assert.equal(
    runInNewContext(miri.match(/^    runs-on: \$\{\{ (.+) \}\}$/m)[1], context),
    "ubuntu-26.04",
  );
});

test("ordinary main prose skips heavy CI while detection failures reach the aggregate", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const changes = indentedBlock(workflow, "changes", 2);
  assert.match(changes, /main-full: \$\{\{ steps\.policy\.outputs\.main-full \}\}/u);
  assert.match(changes, /fetch-depth:.*event_name.*push.*\x270\x27.*\x271\x27/u);
  assert.match(changes, /scripts\/ci-change-policy\.mjs/u);
  for (const name of [
    "check",
    "drift",
    "windows-rust",
    "windows-type-aware",
    "miri",
    "vscode",
    "vscode-package-targets",
    "vscode-target-host",
  ]) {
    const job = indentedBlock(workflow, name, 2);
    assert.match(job, /needs\.changes\.outputs\.main-full != 'false'/u, name);
  }
  const aggregate = indentedBlock(workflow, "ci-ok", 2);
  assert.match(aggregate, /needs: \[changes,/u);
  assert.match(aggregate, /if: always\(\)/u);
  assert.match(aggregate, /result.*!=.*success.*&&.*result.*!=.*skipped/u);
  for (const name of ["typos", "js-lint"]) {
    assert.doesNotMatch(indentedBlock(workflow, name, 2), /main-full/u);
  }
});

test("coverage skips prose computation and publishes only successful fresh artifacts", () => {
  const workflow = readWorkflow(".github/workflows/coverage.yml");
  const changes = indentedBlock(workflow, "changes", 2);
  assert.match(changes, /fetch-depth: 0/u);
  assert.match(changes, /scripts\/ci-change-policy\.mjs/u);
  const coverage = indentedBlock(workflow, "coverage", 2);
  assert.match(coverage, /needs: changes/u);
  assert.match(coverage, /always\(\).*needs\.changes\.outputs\.main-full != 'false'/u);
  assert.match(indentedBlock(workflow, "publish", 2), /needs\.coverage\.result == 'success'/u);
});

test("main scheduling conditions execute release checks and fail closed", async () => {
  const { runInNewContext } = await import("node:vm");
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const context = {
    github: { event_name: "push", event: { head_commit: { message: "chore: release v1.2.3" } } },
    needs: { changes: { outputs: { "main-full": "true" } } },
    startsWith: (value, prefix) => value.startsWith(prefix),
    always: () => true,
    cancelled: () => false,
  };
  const aggregate = indentedBlock(workflow, "ci-ok", 2);
  for (const match of indentedBlock(workflow, "jobs", 0).matchAll(/^ {2}([\w-]+):\n/gmu)) {
    const name = match[1];
    if (["changes", "miri-runner", "heavy-runner", "ci-ok"].includes(name)) continue;
    const job = indentedBlock(workflow, name, 2);
    assert.ok(aggregate.includes(name), `${name} failures must reach CI`);
    const condition = job
      .match(/^    if: (.+)$/mu)?.[1]
      ?.replace(/^"|"$/gu, "")
      .replace(/\.([\w]+-[\w-]+)/gu, '["$1"]');
    if (!condition) continue;
    assert.equal(runInNewContext(condition, context), true, `${name} must run for release`);
    context.needs.changes.outputs["main-full"] = undefined;
    assert.equal(runInNewContext(condition, context), true, `${name} must run with missing policy`);
    context.needs.changes.outputs["main-full"] = "false";
    assert.equal(runInNewContext(condition, context), false, `${name} may skip proven prose`);
    context.needs.changes.outputs["main-full"] = "true";
  }
  const coverage = indentedBlock(readWorkflow(".github/workflows/coverage.yml"), "coverage", 2);
  const condition = coverage.match(/^    if: (.+)$/mu)[1].replace(/\.([\w]+-[\w-]+)/gu, '["$1"]');
  context.needs.changes.result = "failure";
  context.needs.changes.outputs["main-full"] = "false";
  assert.equal(
    runInNewContext(condition, context),
    true,
    "failed coverage detection runs full even with stale false output",
  );
});

test("Miri caches restore on PRs but save only on main", async () => {
  const { runInNewContext } = await import("node:vm");
  const miri = indentedBlock(readWorkflow(".github/workflows/ci.yml"), "miri", 2);
  const cache = miri.slice(miri.indexOf("- uses: Swatinem/rust-cache@"));
  const save = cache.match(/^          save-if: \$\{\{ (.+) \}\}$/m)?.[1];
  assert.ok(save, "Miri cache needs an explicit save policy");
  for (const [ref, expected] of [
    ["refs/heads/main", true],
    ["refs/pull/42/merge", false],
    ["refs/heads/topic", false],
  ]) {
    assert.equal(runInNewContext(save, { github: { ref } }), expected);
  }
  assert.doesNotMatch(
    cache.split("- name: fallow-types")[0],
    /\n\s+if:/,
    "PRs still restore the cache",
  );
});

test("CodSpeed simulation keeps its compatible runner without downgrading walltime", () => {
  const simulationJobs = [];
  const supportedSimulationRunners = new Set(["ubuntu-24.04"]);

  for (const file of readdirSync(".github/workflows")) {
    if (!file.endsWith(".yml")) continue;
    const workflow = readWorkflow(join(".github/workflows", file));
    for (const match of indentedBlock(workflow, "jobs", 0).matchAll(/^ {2}([\w-]+):\n/gmu)) {
      const name = match[1];
      const job = indentedBlock(workflow, name, 2);
      if (!/uses: CodSpeedHQ\/action@/u.test(job) || !/mode: simulation/u.test(job)) continue;
      simulationJobs.push(`${file}/${name}`);
      const runner = job.match(/^    runs-on: (.+)$/mu)?.[1];
      assert.ok(
        supportedSimulationRunners.has(runner),
        `${file}/${name}: CodSpeed simulation needs its known-compatible runner, received ${runner}`,
      );
    }
  }

  assert.ok(simulationJobs.includes("bench.yml/benchmark"));
  assert.ok(simulationJobs.includes("bench.yml/benchmark-full"));
  assert.ok(simulationJobs.includes("bench-cli-instructions.yml/instructions"));
  const benchmark = readWorkflow(".github/workflows/bench.yml");
  for (const name of ["benchmark-harness", "determine-matrix"]) {
    assert.match(indentedBlock(benchmark, name, 2), /runs-on: ubuntu-26\.04/u);
  }
  const walltime = readWorkflow(".github/workflows/bench-type-aware.yml");
  assert.match(walltime, /mode: walltime/u);
  assert.match(walltime, /runs-on: ubuntu-26\.04/u);
});

test("type-aware benchmarks supersede only the same pull request", async () => {
  const { runInNewContext } = await import("node:vm");
  const concurrency = indentedBlock(
    readWorkflow(".github/workflows/bench-type-aware.yml"),
    "concurrency",
    0,
  );
  const group = concurrency.match(/^  group: (.+)$/m)[1];
  const cancel = concurrency.match(/^  cancel-in-progress: \$\{\{ (.+) \}\}$/m)[1];
  const evaluate = (event_name, number, run_id) => {
    const github = {
      workflow: "Type-aware Benchmarks",
      event_name,
      event: { pull_request: { number } },
      run_id,
    };
    return {
      group: group.replace(/\$\{\{ (.+?) \}\}/g, (_, expression) =>
        runInNewContext(expression, { github }),
      ),
      cancel: runInNewContext(cancel, { github }),
    };
  };
  assert.deepEqual(evaluate("pull_request", 42, 100), evaluate("pull_request", 42, 101));
  assert.notEqual(evaluate("pull_request", 42, 100).group, evaluate("pull_request", 43, 101).group);
  assert.equal(evaluate("pull_request", 42, 100).cancel, true);
  for (const event of ["push", "workflow_dispatch"]) {
    assert.equal(evaluate(event, undefined, 100).cancel, false);
    assert.notEqual(evaluate(event, undefined, 100).group, evaluate(event, undefined, 101).group);
    assert.notEqual(evaluate(event, undefined, 100).group, evaluate("pull_request", 42, 100).group);
  }
});

test("failed selector steps discard partial Blacksmith output despite continue-on-error", async () => {
  const { runInNewContext } = await import("node:vm");
  const selector = indentedBlock(readWorkflow(".github/workflows/ci.yml"), "miri-runner", 2);
  const output = selector.match(/^      runner: \$\{\{ (.+) \}\}$/m)?.[1];
  assert.ok(output);
  for (const outcome of ["failure", "skipped", "cancelled"]) {
    assert.equal(
      runInNewContext(output, {
        steps: { runner: { outcome, outputs: { runner: "blacksmith-4vcpu-ubuntu-2404" } } },
      }),
      "ubuntu-26.04",
      outcome,
    );
  }
  assert.equal(
    runInNewContext(output, {
      steps: {
        runner: { outcome: "success", outputs: { runner: "blacksmith-4vcpu-ubuntu-2404" } },
      },
    }),
    "blacksmith-4vcpu-ubuntu-2404",
  );
});

test("workflow environments do not assign an explicit empty Cargo build target", () => {
  const checkTarget = (workflow, path) =>
    assert.doesNotMatch(
      workflow,
      /^[ \t]+CARGO_BUILD_TARGET:[ \t]*(?:''|"")?[ \t]*(?:#.*)?$/m,
      `${path} must omit CARGO_BUILD_TARGET instead of assigning an empty value`,
    );
  const directory = ".github/workflows";
  for (const file of readdirSync(directory).filter((name) => /\.ya?ml$/.test(name))) {
    const path = join(directory, file);
    const workflow = readWorkflow(path);
    checkTarget(workflow, path);
    if (file === "ci.yml") {
      for (const empty of ["", "''", '""']) {
        const invalid = workflow.replace(
          "  check:\n",
          `  check:\n    env:\n      CARGO_BUILD_TARGET: ${empty}\n`,
        );
        assert.throws(() => checkTarget(invalid, path), /must omit CARGO_BUILD_TARGET/);
      }
    }
  }
});

test("heavy jobs admit a reserved runner and retain GitHub fallback after selector failure", async () => {
  const { runInNewContext } = await import("node:vm");
  for (const [file, jobName] of [
    ["ci.yml", "check"],
    ["release-validation.yml", "drift-full"],
  ]) {
    const workflow = readWorkflow(`.github/workflows/${file}`);
    const job = indentedBlock(workflow, jobName, 2);
    const runner = job.match(/^    runs-on: (.+)$/m)[1].replace(/^\$\{\{ | \}\}$/g, "");
    const context = {
      github: { run_attempt: 1, event_name: "push" },
      needs: {
        changes: { outputs: { rust: "true", "main-full": "true" } },
        "heavy-runner": { result: "success", outputs: { runner: "blacksmith-4vcpu-ubuntu-2404" } },
      },
      always: () => true,
      cancelled: () => false,
    };
    const evaluateRunner = () =>
      runner === "ubuntu-26.04" ? runner : runInNewContext(runner, context);
    assert.equal(
      evaluateRunner(),
      "blacksmith-4vcpu-ubuntu-2404",
      `${file} must admit a reserved first attempt`,
    );
    const condition = job.match(/^    if: (.+)$/m)?.[1]?.replace(/\.([\w]+-[\w-]+)/g, '["$1"]');
    assert.ok(condition, `${file} must survive optional selector failure`);
    for (const result of ["failure", "skipped", "cancelled"]) {
      context.needs["heavy-runner"].result = result;
      assert.equal(runInNewContext(condition, context), true);
      assert.equal(evaluateRunner(), "ubuntu-26.04");
    }
    context.needs["heavy-runner"].result = "success";
    context.github.run_attempt = 2;
    assert.equal(evaluateRunner(), "ubuntu-26.04", "retained outputs cannot admit reruns");
    context.github.run_attempt = 1;
    for (const outputs of [{}, { runner: "blacksmith-8vcpu-ubuntu-2404" }]) {
      context.needs["heavy-runner"].outputs = outputs;
      assert.equal(evaluateRunner(), "ubuntu-26.04");
    }
    context.cancelled = () => true;
    assert.equal(runInNewContext(condition, context), false);
    if (file === "ci.yml") {
      context.cancelled = () => false;
      context.github.event_name = "pull_request";
      context.needs.changes.outputs.rust = "false";
      assert.equal(runInNewContext(condition, context), false, "Check retains its path condition");
    }
  }
});

test("heavy selectors skip startup when unset and publish only successful trusted outputs", async () => {
  const { runInNewContext } = await import("node:vm");
  for (const file of ["ci.yml", "release-validation.yml"]) {
    const workflow = readWorkflow(`.github/workflows/${file}`);
    const selector = indentedBlock(workflow, "heavy-runner", 2);
    const condition = selector
      .match(/    if: >-\n([\s\S]+?)\n    runs-on:/)?.[1]
      .trim()
      .replace(/\.([\w]+-[\w-]+)/g, '["$1"]');
    assert.ok(condition, "the selector must gate startup");
    const context = {
      vars: { BLACKSMITH_HEAVY_ALLOCATION: "" },
      github: {
        repository: "fallow-rs/fallow",
        run_attempt: 1,
        actor: "maintainer",
        ref: "refs/heads/main",
        event_name: file === "ci.yml" ? "push" : "workflow_dispatch",
        event: { pull_request: { head: { repo: { full_name: "fallow-rs/fallow" } } } },
      },
      needs: { changes: { outputs: { rust: "true", "main-full": "true" } } },
    };
    assert.equal(
      runInNewContext(condition, context),
      false,
      "no variable means no selector runner startup",
    );
    context.vars.BLACKSMITH_HEAVY_ALLOCATION = "configured";
    assert.equal(runInNewContext(condition, context), true);
    context.github.run_attempt = 2;
    assert.equal(runInNewContext(condition, context), false);
    context.github.run_attempt = 1;
    context.github.actor = "dependabot[bot]";
    assert.equal(runInNewContext(condition, context), false);
    context.github.actor = "maintainer";
    context.github.repository = "fork/fallow";
    assert.equal(runInNewContext(condition, context), false);
    context.github.repository = "fallow-rs/fallow";
    if (file === "ci.yml") {
      context.github.event_name = "pull_request";
      assert.equal(runInNewContext(condition, context), true);
      context.github.event.pull_request.head.repo.full_name = "fork/fallow";
      assert.equal(runInNewContext(condition, context), false);
      context.github.event_name = "push";
      context.needs.changes.outputs.rust = "false";
      context.needs.changes.outputs["main-full"] = "false";
      assert.equal(runInNewContext(condition, context), false);
    } else {
      context.github.event_name = "schedule";
      assert.equal(runInNewContext(condition, context), true);
      context.github.ref = "refs/heads/topic";
      assert.equal(runInNewContext(condition, context), false);
    }
    assert.match(selector, /runs-on: ubuntu-26.04/);
    assert.match(selector, /permissions:\n      contents: read\n/);
    assert.match(selector, /ref: refs\/heads\/main\n          persist-credentials: false/);
    assert.match(selector, /node-version: '22'/);
    assert.match(
      selector,
      /BLACKSMITH_MIRI_ALLOCATION: \$\{\{ vars\.BLACKSMITH_MIRI_ALLOCATION \}\}/,
    );
    assert.match(selector, /if \[ ! -f scripts\/select-heavy-runner\.mjs \]; then/);
    assert.doesNotMatch(selector, /secrets\.|id-token:|contents: write/);
    const output = selector.match(/^      runner: \$\{\{ (.+) \}\}$/m)?.[1];
    assert.ok(output);
    for (const outcome of ["success", "failure", "skipped", "cancelled"]) {
      assert.equal(
        runInNewContext(output, {
          steps: { runner: { outcome, outputs: { runner: "blacksmith-4vcpu-ubuntu-2404" } } },
        }),
        outcome === "success" ? "blacksmith-4vcpu-ubuntu-2404" : "ubuntu-26.04",
      );
    }
  }
});

test("CI aggregate ignores optional selector failure while real Check failure propagates", async () => {
  const { spawnSync } = await import("node:child_process");
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const aggregate = indentedBlock(workflow, "ci-ok", 2);
  const names = aggregate.match(/^    needs: \[([^\]]+)\]/m)[1].split(/,\s*/);
  assert.ok(names.includes("check"));
  assert.ok(
    !names.includes("heavy-runner"),
    "optional selector failures are carried by Check fallback",
  );
  const script = aggregate
    .match(/        run: \|\n([\s\S]+)/)[1]
    .split("\n")
    .map((line) => line.slice(10))
    .join("\n");
  for (const [result, success] of [
    ["success", true],
    ["skipped", true],
    ["failure", false],
    ["cancelled", false],
  ]) {
    const outcomes = names.map((name) => (name === "check" ? result : "success"));
    const run = spawnSync(
      "bash",
      ["-c", script.replace(/\$\{\{ toJSON\(needs\.\*\.result\) \}\}/, JSON.stringify(outcomes))],
      { encoding: "utf8" },
    );
    assert.equal(run.status === 0, success, run.stderr);
  }
  const rustPaths = listedPaths(indentedBlock(indentedBlock(workflow, "changes", 2), "rust", 12));
  for (const path of ["scripts/select-heavy-runner.mjs", "scripts/select-heavy-runner.test.mjs"])
    assert.ok(rustPaths.includes(path), `routing edits must exercise Check: ${path}`);
});

test("heavy admission reserves the actual job timeouts including overhead", async () => {
  const { selectHeavyRunner } = await import("./select-heavy-runner.mjs");
  for (const [workflow, job, event] of [
    ["ci.yml", "check", "push"],
    ["release-validation.yml", "drift-full", "schedule"],
    ["release.yml", "drift-full", "workflow_dispatch"],
  ]) {
    const source = readWorkflow(
      `.github/workflows/${workflow === "release.yml" ? "release-validation.yml" : workflow}`,
    );
    const minutes = Number(indentedBlock(source, job, 2).match(/^    timeout-minutes: (\d+)$/m)[1]);
    const environment = {
      GITHUB_REPOSITORY: "fallow-rs/fallow",
      GITHUB_EVENT_NAME: event,
      GITHUB_REF: "refs/heads/main",
      GITHUB_WORKFLOW_REF: `fallow-rs/fallow/.github/workflows/${workflow}@refs/heads/main`,
      GITHUB_RUN_ATTEMPT: "1",
      GITHUB_RUN_NUMBER: "1",
      GITHUB_ACTOR: "maintainer",
      HEAVY_JOB: job,
    };
    const reservation = {
      month: "2026-09",
      budgetCredits: minutes * 2 + 30,
      priorReservedCredits: 0,
      allocations: [{ workflow, job, firstRunNumber: 1, slots: 1 }],
    };
    assert.equal(
      selectHeavyRunner(
        { ...environment, BLACKSMITH_HEAVY_ALLOCATION: JSON.stringify(reservation) },
        new Date("2026-09-15T00:00:00Z"),
      ),
      "blacksmith-4vcpu-ubuntu-2404",
    );
    reservation.budgetCredits -= 1;
    assert.equal(
      selectHeavyRunner(
        { ...environment, BLACKSMITH_HEAVY_ALLOCATION: JSON.stringify(reservation) },
        new Date("2026-09-15T00:00:00Z"),
      ),
      "ubuntu-26.04",
    );
  }
});

// The Hawk workflow installs one toolchain and `check-hawk.sh` runs Hawk with
// `cargo +<version>`. When the two drift, the job either fails on a missing
// toolchain or lets rustup install the old one without notice.
test("the Hawk workflow installs the toolchain that check-hawk.sh runs", () => {
  const workflow = readWorkflow(".github/workflows/hawk.yml");
  const script = readFileSync("scripts/check-hawk.sh", "utf8");
  const installed = workflow.match(/^\s+toolchain:\s*'([^']+)'/m)?.[1];
  const used = new Set([...script.matchAll(/cargo \+([\w.-]+) hawk/g)].map((match) => match[1]));

  assert.ok(installed, "hawk.yml must pin a toolchain for setup-rust");
  assert.deepEqual(
    [...used],
    [installed],
    "check-hawk.sh must run Hawk on the toolchain that hawk.yml installs",
  );
});
