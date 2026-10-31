import assert from "node:assert/strict";
import {
  existsSync,
  readdirSync,
  readFileSync,
  mkdirSync,
  mkdtempSync,
  copyFileSync,
  writeFileSync,
  rmSync,
} from "node:fs";
import { spawnSync as runProcess } from "node:child_process";
import { tmpdir } from "node:os";
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
  assert.match(job, /-dict="\$PWD\/fuzz\/dicts\/\$\{target\}\.dict"/);
  assert.match(job, /if ! cargo[\s\S]*failed=1[\s\S]*exit "\$failed"/);
  assert.match(job, /actions\/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9/);
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
    "crates/engine/src/codeowners.rs",
    "rust-toolchain.toml",
    ".github/workflows/fuzz-smoke.yml",
  ]) {
    assert.ok(pushPaths.includes(path), `fuzz push filter is missing ${path}`);
  }
});

test("branch coverage reports weekly on the Fuzz Smoke nightly without gating releases", async () => {
  const workflow = readWorkflow(".github/workflows/branch-coverage.yml");
  const fuzzJob = indentedBlock(readWorkflow(".github/workflows/fuzz-smoke.yml"), "fuzz-smoke", 2);
  const triggers = indentedBlock(workflow, "on", 0);
  const job = indentedBlock(workflow, "branch-coverage", 2);
  const { REQUIRED_WORKFLOWS } = await import("./verify-release-ci.mjs");
  const fuzzNightly = fuzzJob.match(/toolchain: (nightly-\d{4}-\d{2}-\d{2})/)?.[1];

  assert.ok(fuzzNightly, "Fuzz Smoke must pin a dated nightly");
  assert.match(workflow, new RegExp(`^  NIGHTLY_TOOLCHAIN: ${fuzzNightly}$`, "m"));
  assert.match(triggers, /^  schedule:\n    - cron: '20 4 \* \* 1'$/m);
  assert.match(triggers, /^  workflow_dispatch:$/m);
  assert.doesNotMatch(triggers, /^  (push|pull_request|pull_request_target):/m);
  assert.ok(
    !REQUIRED_WORKFLOWS.some(({ file }) => file === "branch-coverage.yml"),
    "a report-only scheduled workflow must not gate releases",
  );
  assert.match(workflow, /^permissions: \{\}$/m);
  assert.match(job, /permissions:\n      contents: read/);
  assert.match(job, /persist-credentials: false/);
  assert.match(job, /toolchain: \$\{\{ env\.NIGHTLY_TOOLCHAIN \}\}/);
  assert.match(job, /cargo \+"\$NIGHTLY_TOOLCHAIN" llvm-cov --workspace --branch --lcov/);
  assert.match(job, /\/\^BRF:\/[\s\S]*\/\^BRH:\/[\s\S]*GITHUB_STEP_SUMMARY/);
  assert.doesNotMatch(job, /exit 1|FLOOR=/, "branch coverage only reports and has no floor");
});

test("public skill PR parity uses the exact base while push and candidate checks stay strict", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const job = indentedBlock(workflow, "skills-vendor", 2);
  const lintJob = indentedBlock(workflow, "check-lint", 2);
  const jsJob = indentedBlock(workflow, "js-lint", 2);
  const aggregate = indentedBlock(workflow, "ci-ok", 2);
  const rustPaths = listedPaths(indentedBlock(workflow, "rust", 12));

  assert.match(
    job,
    /name: Check out public base skill contract\n\s+if: github\.event_name == 'pull_request'\n\s+uses: actions\/checkout@[^\n]+\n\s+with:\n\s+repository: \$\{\{ github\.repository \}\}\n\s+ref: \$\{\{ github\.event\.pull_request\.base\.sha \}\}\n\s+path: \.fallow-skills-base\n\s+persist-credentials: false/u,
  );
  assert.match(
    job,
    /working-directory: \$\{\{ github\.event_name == 'pull_request' && '\.fallow-skills-base' \|\| '\.' \}\}/u,
  );
  assert.match(job, /run: node scripts\/vendor-skills\.mjs --check/u);
  assert.match(job, /FALLOW_SKILLS_DIR: \$\{\{ github\.workspace \}\}\/\.fallow-skills-src/u);
  assert.doesNotMatch(job, /continue-on-error: true|contents: write|id-token: write/u);
  assert.match(aggregate, /needs: \[[^\n]*skills-vendor/u);
  assert.ok(rustPaths.includes("npm/fallow/skills/fallow/**"));
  assert.ok(rustPaths.includes("npm/fallow/skills/fallow-setup/**"));
  assert.match(lintJob, /run: CI=true npm run generate:contracts:check/u);
  assert.match(jsJob, /run: npm run check:agent-adapters/u);
});

test("bundled skill validation uses the root lockfile without network fallback", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const npmPackageJob = indentedBlock(workflow, "npm-package", 2);
  const rootPackage = JSON.parse(readFileSync("package.json", "utf8"));
  const nestedPackage = JSON.parse(readFileSync("npm/fallow/package.json", "utf8"));
  const lockfile = JSON.parse(readFileSync("package-lock.json", "utf8"));

  assert.equal(rootPackage.devDependencies["@tanstack/intent"], "0.5.6");
  assert.equal(
    nestedPackage.devDependencies?.["@tanstack/intent"],
    rootPackage.devDependencies["@tanstack/intent"],
  );
  assert.equal(lockfile.packages[""].devDependencies["@tanstack/intent"], "0.5.6");
  assert.equal(lockfile.packages["node_modules/@tanstack/intent"].version, "0.5.6");
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
  const windowsDriftJob = indentedBlock(workflow, "windows-drift", 2);
  const windowsTypeAwareJob = indentedBlock(workflow, "windows-type-aware", 2);
  const vscodePackageTargetsJob = indentedBlock(workflow, "vscode-package-targets", 2);
  const vscodeTargetHostJob = indentedBlock(workflow, "vscode-target-host", 2);
  const zedJob = indentedBlock(workflow, "zed", 2);
  const aggregateJob = indentedBlock(workflow, "ci-ok", 2);
  const workflowWithoutWindowsJobs = workflow
    .replace(windowsRustJob, "")
    .replace(windowsDriftJob, "")
    .replace(windowsTypeAwareJob, "")
    .replace(vscodePackageTargetsJob, "")
    .replace(vscodeTargetHostJob, "");

  assert.doesNotMatch(workflowWithoutWindowsJobs, /windows-latest|windows-11-arm|macos-latest/);
  for (const job of [checkJob, indentedBlock(workflow, "check-lint", 2)]) {
    assert.match(job, /runs-on:.*\|\| \x27ubuntu-26.04\x27/);
    assert.match(job, /timeout-minutes: 30/);
    assert.doesNotMatch(job, /matrix\.(?!partition\b)|windows-latest|macos-latest/);
  }
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
  // Release Validation runs the drift harness on Linux only, so a harness
  // change must run on Windows on the pull request.
  assert.ok(windowsRustPaths.includes("crates/cli/tests/drift/**"));
  // The drift harness has its own Windows job with the same trigger, so it
  // runs in parallel with the Windows test job.
  assert.match(windowsDriftJob, /needs: changes/);
  assert.equal(
    windowsDriftJob.match(/^    if: (.+)$/m)?.[1],
    windowsRustJob.match(/^    if: (.+)$/m)?.[1],
  );
  assert.match(windowsDriftJob, /runs-on: windows-latest/);
  assert.match(windowsDriftJob, /^[ \t]+run: cargo build -p fallow-mcp$/m);
  assert.match(
    windowsDriftJob,
    /^[ \t]+run: cargo test -p fallow-cli --test drift -- --include-ignored$/m,
  );
  assert.doesNotMatch(windowsRustJob, /--test drift|cargo build -p fallow-mcp/);
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
  assert.match(aggregateJob, /windows-drift/);
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
  assert.match(msrvJob, /^\s+run: cargo check --workspace --locked --offline$/mu);
});

test("MSRV CI fetches locked dependencies with patched Cargo before offline compilation", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const msrvJob = indentedBlock(workflow, "msrv", 2);
  const fetch = "cargo +1.97.1 fetch --locked";
  const compile = "cargo check --workspace --locked --offline";

  assert.match(msrvJob, /rustup toolchain install 1\.97\.1 --profile minimal --no-self-update/);
  assert.ok(msrvJob.includes(fetch), "patched Cargo fetches the existing lockfile");
  assert.ok(msrvJob.indexOf(fetch) < msrvJob.indexOf(compile), "fetch runs before compilation");
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
  assert.match(job, /cargo test --workspace --no-fail-fast --lib --bins --tests --examples/);
  assert.match(job, /cargo test -p fallow-cli --no-fail-fast --features test-sidecar-key/);
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

test("release runs the test suite natively on arm64 Linux and Apple Silicon", () => {
  const validationWorkflow = readWorkflow(".github/workflows/release-validation.yml");
  const job = indentedBlock(validationWorkflow, "arm64-verify", 2);

  // The release cross-compiles the arm64 binaries, so this job is the only
  // native arm64 test run before publication.
  assert.match(job, /os: \[ubuntu-24\.04-arm, macos-15\]/);
  assert.match(job, /fail-fast: false/);
  assert.match(job, /permissions:\n\s+contents: read/);
  assert.doesNotMatch(job, /id-token: write|contents: write|secrets\./);
  assert.match(job, /cargo test --workspace --no-fail-fast --lib --bins --tests --examples/);
  assert.match(job, /cd crates\/napi && npm test/);
});

test("CodeQL analyze jobs always report so the required checks resolve", () => {
  const workflow = readWorkflow(".github/workflows/codeql.yml");
  const job = indentedBlock(workflow, "analyze", 2);

  // A job that a job-level `if` skips leaves a required check waiting
  // forever, so only the steps may skip.
  assert.doesNotMatch(job, /^ {4}if:/mu);
  assert.match(job, /language: \[rust, javascript-typescript, actions\]/u);
  assert.match(
    job,
    /ANALYZE: \$\{\{ matrix\.language != 'rust' \|\| github\.event_name != 'pull_request' \|\| needs\.changes\.outputs\.rust == 'true' \}\}/u,
  );
  assert.match(job, /codeql-action\/analyze@[0-9a-f]{40}[^\n]*\n\s+if: env\.ANALYZE == 'true'/u);
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

test("release runs the shipped musl binaries in Alpine before publication", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const job = indentedBlock(workflow, "musl-smoke", 2);
  const gate = indentedBlock(workflow, "release-verified", 2);
  const build = indentedBlock(workflow, "build", 2);
  const script = readFileSync("scripts/musl-smoke.sh", "utf8");
  const legs = Array.from(
    job.matchAll(/- target: (\S+)\n\s+npm_dir: (\S+)\n\s+os: (\S+)\n\s+platform: (\S+)/gu),
    (match) => match.slice(1),
  );

  assert.deepEqual(legs, [
    ["x86_64-unknown-linux-musl", "linux-x64-musl", "ubuntu-26.04", "linux/amd64"],
    ["aarch64-unknown-linux-musl", "linux-arm64-musl", "ubuntu-24.04-arm", "linux/arm64"],
  ]);
  for (const [target, npmDir] of legs) {
    assert.match(build, new RegExp(`target: ${target}\\n[\\s\\S]*?npm_dir: ${npmDir}\\n`, "u"));
  }
  assert.match(job, /^    if: \$\{\{ !inputs.auth_check \}\}$/mu);
  assert.match(job, /^    needs: build$/mu);
  assert.match(job, /^    runs-on: \$\{\{ matrix\.os \}\}$/mu);
  assert.match(job, /^    permissions:\n {6}contents: read\n {4}strategy:/mu);
  assert.doesNotMatch(job, /secrets\.|id-token|: write|environment:/u);
  assert.match(job, /persist-credentials: false/u);
  assert.match(job, /^ {6}ALPINE_IMAGE: alpine@sha256:[0-9a-f]{64}$/mu);
  assert.match(job, /# alpine:\d+\.\d+\.\d+, multi-arch index digest/u);
  assert.match(job, /name: unsigned-binaries-\$\{\{ matrix\.npm_dir \}\}/u);
  assert.match(job, /docker run --rm --network none --platform "\$FALLOW_PLATFORM"/u);
  assert.match(job, /\/smoke\/bin:ro/u);
  assert.match(job, /tests\/fixtures\/basic-project:\/smoke\/fixture:ro/u);
  assert.match(job, /"\$ALPINE_IMAGE" \\\n\s+sh \/smoke\/musl-smoke\.sh /u);
  assert.match(job, /jq -e 'type == "object"'/u);
  assert.match(job, /for report in dead-code dead-code-multicall similar-code-status; do/u);
  assert.doesNotMatch(
    job,
    /FALLOW_SMOKE_SKIP_MULTICALL/u,
    "release runs must test the multicall binary",
  );
  assert.doesNotMatch(job, /cargo |npm (?:ci|install)|pnpm/u);
  assert.match(gate, /needs: \[[^\]]*\bmusl-smoke\b[^\]]*\]/u);

  assert.match(script, /^#!\/bin\/sh$/mu);
  assert.doesNotMatch(script, /^#!\/.*bash/mu, "Alpine has no bash");
  for (const check of [
    /check_version "fallow" "fallow"/u,
    /check_analysis "fallow" "dead-code"/u,
    /check_version "fallow-mcp" "fallow-mcp"/u,
    /check_version "fallow-lsp" "fallow-lsp"/u,
    /check_version "fallow-multicall mcp-server" "fallow-mcp"/u,
    /check_version "fallow-multicall lsp-server" "fallow-lsp"/u,
    /^stage fallow-similar-code$/mu,
    /check_version "fallow-similar-code" "fallow-similar-code"/u,
    /^check_similar_code_status$/mu,
    /"\$work\/bin\/fallow-similar-code" status --json/u,
    /"model_ready":false/u,
    /dead-code --format json --quiet/u,
    /"\$status" -ne 0 \] && \[ "\$status" -ne 1 \]/u,
  ]) {
    assert.match(script, check);
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
  assert.match(similarCodeConformance, /needs: \[build, sign\]/);
  assert.match(similarCodeConformance, /permissions:\n\s+contents: read/);
  assert.doesNotMatch(similarCodeConformance, /id-token: write|contents: write/);
  // The macOS binaries link Apple Accelerate, a separate numeric path from the
  // Linux binary, so each one runs the conformance check on its own platform.
  const conformanceLegs = Array.from(
    similarCodeConformance.matchAll(/^\s+- npm_dir: (\S+)\n\s+os: (\S+)$/gmu),
    (match) => `${match[1]}@${match[2]}`,
  );
  assert.deepEqual(conformanceLegs.toSorted(), [
    "darwin-arm64@macos-latest",
    "darwin-x64@macos-15-intel",
    "linux-x64-gnu@ubuntu-26.04",
  ]);
  assert.match(similarCodeConformance, /runs-on: \$\{\{ matrix\.os \}\}/u);
  assert.match(similarCodeConformance, /name: fallow-similar-code-\$\{\{ matrix\.npm_dir \}\}/u);
  assert.doesNotMatch(similarCodeConformance, /stat --format/u);
  assert.match(similarCodeConformance, /verify-binary\.mjs/);
  assert.match(similarCodeConformance, /release_binary_max_bytes/);
  // The dependency audit lives in release validation, which `validate` calls.
  assert.match(validate, /uses: \.\/\.github\/workflows\/release-validation\.yml/u);
  assert.doesNotMatch(similarCodeConformance, /cargo audit|cargo deny|cargo-audit|cargo-deny/u);
  const sidecarAuditJob = indentedBlock(
    readWorkflow(".github/workflows/release-validation.yml"),
    "similar-code-audit",
    2,
  );
  assert.match(sidecarAuditJob, /permissions:\n\s+contents: read/u);
  assert.match(sidecarAuditJob, /persist-credentials: false/u);
  assert.doesNotMatch(sidecarAuditJob, /id-token: write|contents: write|secrets\./u);
  assert.match(sidecarAuditJob, /check-similar-code-sidecar-audit\.mjs/);
  const sidecarAudit = /cargo audit \\\n((?:\s+--[^\n]+\n)+)/u.exec(sidecarAuditJob);
  assert.ok(sidecarAudit, "release validation must run cargo audit for the sidecar");
  assert.match(sidecarAudit[1], /--file tools\/similar-code-sidecar\/Cargo\.lock/u);
  assert.match(sidecarAudit[1], /--deny unmaintained/u);
  assert.deepEqual(
    [...sidecarAudit[1].matchAll(/--ignore (\S+)/gu)].map((match) => match[1]),
    ["RUSTSEC-2024-0436", "RUSTSEC-2026-0367", "RUSTSEC-2026-0368", "RUSTSEC-2026-0369"],
  );
  assert.match(
    sidecarAuditJob,
    /cargo deny[\s\S]*tools\/similar-code-sidecar\/Cargo\.toml[\s\S]*check licenses/u,
  );
  assert.match(similarCodeConformance, /semantic-clone-candle-conformance\.mjs/);
  assert.match(gate, /needs: \[build, sign, validate, similar-code-conformance, musl-smoke\]/);
  assert.match(gate, /permissions: \{\}/);
  assert.match(publishCrates, /needs: \[release-verified, release-assets\]/);
  assert.match(releaseAssets, /needs: \[release-verified, vscode-prep, vscode-host-smoke, sbom\]/);
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
      /needs: \[vscode-prep, vscode-host-smoke, release-assets, npm-prep, npm-root-approved\]/,
      registry,
    );
    assert.match(job, /permissions:\n\s+contents: read/, registry);
    const directory = cli === "ovsx" ? "open-vsx" : "marketplace";
    const publisher = JSON.parse(
      readFileSync(`.github/publishers/${directory}/package.json`, "utf8"),
    );
    assert.equal(publisher.dependencies[cli], pin, registry);
    assert.match(job, /npm ci[^\n]+--ignore-scripts --no-audit --no-fund/u, registry);
    assert.match(job, new RegExp(`secrets\\.${secret}`, "u"), registry);
    assert.doesNotMatch(job, new RegExp(`secrets\\.${otherSecret}`, "u"), registry);
    assert.doesNotMatch(job, /pnpm|npm install|\bbuild\b/u, registry);
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
    assert.match(job, /^ {4}timeout-minutes: 60$/mu, registry);
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

const WAIT_STEP = "Wait until fallow@version is public with the staged bytes";

/** Returns the shell body of the public root wait step of a job. */
const waitStepBody = (job) => {
  const start = job.indexOf(`- name: ${WAIT_STEP}`);
  if (start === -1) return null;
  const step = job.slice(start).split(/\n {6}- |\n\n/u)[0];
  return /run: \|\n([\s\S]*)$/u.exec(step)?.[1].trimEnd() ?? null;
};

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
  // A rerun after the stage approval finds the root public and skips it.
  // The digest must still reach the gate, or the wait fails with an empty
  // expected sha256 and no rerun can recover the run (v3.38.0).
  const digestOutput = npmPublish.indexOf('echo "fallow_sha256=$digest" >> "$GITHUB_OUTPUT"');
  const publishedSkip = npmPublish.indexOf('if npm view "$name@$version" version');
  assert.notEqual(publishedSkip, -1, "npm-publish must skip a published tarball");
  assert.ok(
    digestOutput < publishedSkip,
    "npm-publish must write the root digest before it skips a published tarball",
  );
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
  const releaseReady = indentedBlock(workflow, "release-ready", 2);
  assert.match(releaseReady, /needs: \[[^\]]*\bnpm-root-approved\b[^\]]*\]/u);
  const gateWait = waitStepBody(gate);
  assert.ok(gateWait, "npm-root-approved must run the public root wait");
  assert.doesNotMatch(gateWait, /Waiting for the maintainer/u, "the wait body stays neutral");
  const announce = gate.indexOf("- name: Ask for the npm approval of the staged fallow root");
  assert.ok(announce !== -1 && announce < gate.indexOf(`- name: ${WAIT_STEP}`));
  assert.match(gate.slice(announce), /Verify the stage against sha256/u);
  for (const publisher of [marketplace, openVsx]) {
    // npm-root-approved is the only job that waits for the npm approval. The
    // publishers need it, so they hold no runner while the approval is
    // pending. Each publisher still checks the public bytes itself before it
    // uploads anything, with a short bound instead of a long poll.
    assert.match(
      publisher,
      /needs: \[vscode-prep, vscode-host-smoke, release-assets, npm-prep, npm-root-approved\]/u,
    );
    assert.doesNotMatch(publisher, /needs: \[[^\]]*\bnpm-publish\b/u);
    assert.match(publisher, /^ {4}timeout-minutes: 60$/mu);
    assert.equal(waitStepBody(publisher), gateWait, "publishers reuse the exact wait logic");
    const steps = publisher.split(/^ {6}- /mu).slice(1);
    const waitIndex = steps.findIndex((step) => step.startsWith(`name: ${WAIT_STEP}`));
    assert.ok(waitIndex > 0, "a publisher must wait for the approved root");
    for (const [index, step] of steps.entries()) {
      if (index >= waitIndex) break;
      assert.doesNotMatch(step, /secrets\.|_PAT|azure\/login|actions\/checkout|npm ci/u, step);
    }
    const digest = steps.slice(0, waitIndex).join("");
    assert.match(digest, /name: npm-tarballs/u);
    assert.match(digest, /sha256sum "\$NPM_TARBALLS\/20-cli-root\/fallow-\$\{VERSION\}\.tgz"/u);
    assert.match(digest, /EXPECTED_SHA256=\$digest" >> "\$GITHUB_ENV"/u);
    assert.match(steps[waitIndex], /WAIT_MINUTES: '[1-9]'/u);
  }
  assert.match(security, /Publish no VSIX before the approved `fallow` root is public/u);
  assert.match(security, /`npm-root-approved` is the only job that waits/u);
  assert.match(procedure, /Wait for the approved fallow root/u);
  assert.match(procedure, /Do this right after\s+`release-ready` without other work in between/u);
});

test("release procedure dispatches the identity checks only when an input changed", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const procedure = readFileSync("docs/development/release-procedure.md", "utf8");
  const flat = procedure.replace(/\s+/gu, " ");

  // A waiting auth_check run holds this group, so the publication dispatch queues behind it.
  assert.match(workflow, /^concurrency:\n {2}group: release\n {2}cancel-in-progress: false$/mu);
  assert.match(flat, /Dispatch the identity checks only when/u);
  for (const trigger of [
    /a publisher binding/u,
    /a trusted-publisher configuration/u,
    /an environment secret or variable/u,
    /a release credential/u,
    /authentication failure/u,
  ]) {
    assert.match(flat, trigger);
  }
  assert.match(flat, /auth_check=true/u);
  assert.match(flat, /verify-marketplace-identity\.yml/u);
  assert.match(flat, /holds the `release` concurrency group/u);
  assert.match(flat, /approve or cancel it before the publication dispatch/u);
});

test("release credential jobs run in the main-only release environment", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const procedure = readFileSync("docs/development/release-procedure.md", "utf8");
  const security = readFileSync("docs/development/release-security.md", "utf8");
  const credentialJobs = [
    "npm-publish",
    "publish-crates",
    "publisher-auth-check",
    "sign",
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
    const inSigningEnvironment =
      name === "authenticode" && /^ {4}environment: windows-signing$/mu.test(job);
    const holdsCredentials =
      /\$\{\{\s*secrets\.(?!GITHUB_TOKEN\b)/u.test(job) || /^\s+id-token: write$/mu.test(job);

    if (inEnvironment) {
      environmentJobs.push(name);
    }
    assert.ok(
      !holdsCredentials || inEnvironment || inSigningEnvironment,
      `${name} holds publication credentials outside the release environment`,
    );
    if (!inSigningEnvironment) {
      assert.doesNotMatch(job, /^ {4}environment:(?! release$)/mu);
    }
  }

  assert.deepEqual(environmentJobs.toSorted(), credentialJobs);
  assert.match(procedure, /environments\/release\/deployment-branch-policies/u);
  assert.match(procedure, /environments\/windows-signing\/deployment-branch-policies/u);
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
    /name: Run coverage producer conformance\n\s+if: matrix\.partition == 1\n(?:\s+#.*\n)*\s+run: npm run check:coverage-producers/,
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

test("every CI push run on main finishes so the README badge shows a result", async () => {
  const { runInNewContext } = await import("node:vm");
  const concurrency = indentedBlock(readWorkflow(".github/workflows/ci.yml"), "concurrency", 0);
  const group = concurrency.match(/^  group: (.+)$/m)[1];
  const cancel = concurrency.match(/^  cancel-in-progress: \$\{\{ (.+) \}\}$/m)[1];
  const evaluate = (event_name, run_id) => {
    const github = { event_name, ref: "refs/heads/main", run_id };
    return {
      group: group.replace(/\$\{\{ (.+?) \}\}/g, (_, expression) =>
        runInNewContext(expression, { github }),
      ),
      cancel: runInNewContext(cancel, { github }),
    };
  };
  assert.equal(evaluate("pull_request", 100).cancel, true);
  assert.equal(evaluate("pull_request", 100).group, evaluate("pull_request", 101).group);
  for (const event of ["push", "workflow_dispatch", "schedule"]) {
    assert.equal(evaluate(event, 100).cancel, false);
    assert.notEqual(evaluate(event, 100).group, evaluate(event, 101).group);
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

test("CI runs the mutation runner tests at the declared runtime floors", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const job = indentedBlock(workflow, "mutation-runner-floor", 2);
  const protocol = JSON.parse(readFileSync("crates/api/mutation-runner-protocol.json", "utf8"));
  const runners = new Set(protocol.runners.map((runner) => runner.name));

  // The job reads each floor from the protocol, so a floor change needs no
  // workflow change.
  for (const runner of ["node-test", "bun-test"]) {
    assert.ok(runners.has(runner), `the protocol must declare ${runner}`);
    assert.match(job, new RegExp(`floor ${runner}\\)`, "u"));
  }
  assert.match(job, /node-version: \$\{\{ steps\.floor\.outputs\.node \}\}/u);
  assert.match(job, /bun-version: \$\{\{ steps\.floor\.outputs\.bun \}\}/u);
  assert.match(job, /test "\$\(node --version\)" = "v\$NODE_FLOOR"/u);
  assert.match(job, /test "\$\(bun --version\)" = "\$BUN_FLOOR"/u);
  // Without Bun the bun-test suite skips. The job must fail instead.
  assert.match(
    job,
    /FALLOW_REQUIRE_BUN: '1'\n\s+run: node --test tools\/mutation-runner\/test\/\*\.test\.mjs/u,
  );
  assert.match(job, /persist-credentials: false/u);
  assert.match(indentedBlock(workflow, "ci-ok", 2), /mutation-runner-floor/u);
});

test("CI runs clippy on the similar-code sidecar workspace", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const job = indentedBlock(workflow, "similar-code-sidecar", 2);
  const paths = listedPaths(indentedBlock(workflow, "similar-code-sidecar", 12));

  // The sidecar is a separate Cargo workspace, so the workspace clippy run
  // in check-lint does not lint it.
  assert.match(
    job,
    /run: cargo clippy --locked --manifest-path tools\/similar-code-sidecar\/Cargo\.toml --target-dir target --all-targets -- -D warnings/u,
  );
  assert.match(job, /uses: \.\/\.github\/actions\/setup-rust\n\s+with:\n\s+components: clippy/u);
  assert.match(job, /needs\.changes\.outputs\.similar-code-sidecar == 'true'/u);
  assert.match(job, /persist-credentials: false/u);
  // build.rs reads the protocol file, so a change there must lint too.
  for (const path of [
    "tools/similar-code-sidecar/**",
    "crates/api/similar-code-protocol.json",
    "rust-toolchain.toml",
  ]) {
    assert.ok(paths.includes(path), `the similar-code-sidecar filter must list ${path}`);
  }
  assert.match(
    indentedBlock(workflow, "outputs", 4),
    /similar-code-sidecar: \$\{\{ steps\.filter\.outputs\.similar-code-sidecar \}\}/u,
  );
  assert.match(indentedBlock(workflow, "ci-ok", 2), /needs: \[[^\n]*\bsimilar-code-sidecar\b/u);
});

test("the check job runs the end-to-end mutation tests instead of a skip", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const checkJob = indentedBlock(workflow, "check", 2);
  const rustPaths = listedPaths(indentedBlock(workflow, "rust", 12));
  const install = checkJob.indexOf(
    "npm ci --prefix tools/mutation-runner --no-audit --no-fund --ignore-scripts",
  );
  const bun = checkJob.indexOf("uses: oven-sh/setup-bun@");
  const tests = checkJob.indexOf("name: Run tests");

  assert.ok(install > 0, "the check job must install the mutation runner dependencies");
  assert.ok(bun > 0, "the check job must install Bun for the bun-test end-to-end test");
  assert.ok(install < tests && bun < tests, "the installs must come before the tests");
  // Without the env var, a missing install skips each test and CI stays green.
  assert.match(
    checkJob,
    /FALLOW_MUTATION_E2E_REQUIRED: '1'\n\s+run: cargo nextest run --profile ci --workspace/u,
  );
  for (const file of [
    "tools/mutation-runner/src/companion.mjs",
    "tools/mutation-runner/package.json",
    "tools/mutation-runner/package-lock.json",
    "tools/mutation-runner/test/fixtures/vitest-basic/vitest.config.mjs",
    "crates/api/src/mutation/e2e_tests.rs",
  ]) {
    assert.ok(matchesListedPath(rustPaths, file), `the rust filter must include ${file}`);
  }
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
  // A version from an expression comes from another source of truth, such as
  // the runtime floors of the mutation runner protocol.
  const ciNodeVersions = Array.from(
    readWorkflow(join(workflowDir, "ci.yml")).matchAll(/node-version: ['"]?(?!\$\{\{)([^'"\s]+)/gu),
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
// The instrumented build skips LTO through rustflags only. Cargo hashes the
// profile LTO value into the crate symbol names, so a profile override makes
// the profile match almost no function.
const PGO_GENERATE_RUSTFLAGS =
  "rustflags = ['-Cprofile-generate=/tmp/pgo-raw', '-Clto=off', '-Clinker-plugin-lto=no']";
const PROFILE_LTO_OVERRIDE = /profile\.release\.lto|CARGO_PROFILE_RELEASE_LTO/u;
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
  // Every binary compiles C on musl (mimalloc, plus QuickJS in MCP and
  // multicall). The glibc cross-gcc cannot link C into an aarch64 musl binary.
  const aarch64MuslSteps = workflowSteps(build).filter(
    (step) =>
      /if: matrix\.target == 'aarch64-unknown-linux-musl'/u.test(step) &&
      /-p fallow-(?:cli|lsp|mcp|multicall)\b/u.test(step),
  );
  assert.equal(aarch64MuslSteps.length, 4);
  for (const step of aarch64MuslSteps) {
    assert.match(step, /run: cargo zigbuild\b/u, "aarch64-musl C code needs zig");
    assert.doesNotMatch(step, /CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER/u);
  }
  assert.deepEqual(
    aarch64MuslSteps.map((step) => step.match(/-p (fallow-[a-z]+)/u)[1]).toSorted(),
    PGO_RUST_BINARY_PACKAGES,
  );
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
  assert.ok(
    profileJob.includes(PGO_GENERATE_RUSTFLAGS),
    "the instrumented build must skip LTO through rustflags",
  );
  assert.doesNotMatch(
    profileJob,
    PROFILE_LTO_OVERRIDE,
    "a profile LTO override breaks the profile match",
  );
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
  assert.ok(train.includes(PGO_GENERATE_RUSTFLAGS), "the training build must equal release.yml");
  assert.doesNotMatch(
    train,
    PROFILE_LTO_OVERRIDE,
    "a profile LTO override breaks the profile match",
  );
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

// The Blacksmith selectors are steps of the `changes` job. These helpers read
// one selector step and evaluate expressions that use hyphenated keys.
const changesStep = (workflow, name) => {
  const changes = indentedBlock(workflow, "changes", 2);
  const start = changes.indexOf(`      - name: ${name}\n`);
  assert.ok(start >= 0, `Detect changes must run "${name}"`);
  const next = changes.indexOf("\n      - ", start + 1);
  return changes.slice(start, next === -1 ? undefined : next + 1);
};
const bracketKeys = (expression) => expression.replace(/\.([\w]+-[\w-]+)/g, '["$1"]');
const stepCondition = (step) =>
  bracketKeys(step.match(/        if: >-\n([\s\S]+?)\n        [a-z-]+:/)?.[1].trim() ?? "");

test("Miri falls back to GitHub when runner selection fails or a job is rerun", async () => {
  const { runInNewContext } = await import("node:vm");
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const miri = indentedBlock(workflow, "miri", 2);
  assert.match(miri, /^    needs: changes$/m, "Miri must not wait for a selector job");
  const condition = miri.match(/^    if: (.+)$/m)?.[1];
  assert.match(condition, /always\(\)/, "a failed detection must not skip Miri");
  assert.match(condition, /!cancelled\(\)/);
  const runner = bracketKeys(miri.match(/^    runs-on: \$\{\{ (.+) \}\}$/m)?.[1] ?? "");
  assert.ok(runner, "Miri must select a runner with GitHub fallback");
  const context = {
    github: { run_attempt: 1, event_name: "push" },
    needs: {
      changes: { outputs: { miri: "true", "miri-runner": "blacksmith-4vcpu-ubuntu-2404" } },
    },
    always: () => true,
    cancelled: () => false,
  };
  assert.equal(runInNewContext(condition, context), true);
  assert.equal(runInNewContext(runner, context), "blacksmith-4vcpu-ubuntu-2404");
  context.needs.changes.outputs["miri-runner"] = "ubuntu-26.04";
  assert.equal(runInNewContext(runner, context), "ubuntu-26.04", "the fallback output");
  context.needs.changes.outputs["miri-runner"] = "blacksmith-4vcpu-ubuntu-2404";
  context.github.run_attempt = 2;
  assert.equal(
    runInNewContext(runner, context),
    "ubuntu-26.04",
    "rerun-failed-jobs may reuse old selector output",
  );
  context.github.run_attempt = 1;
  context.needs.changes.outputs["miri-runner"] = undefined;
  assert.equal(runInNewContext(runner, context), "ubuntu-26.04", "missing output falls back");
  context.needs.changes.outputs["miri-runner"] = "unexpected-runner";
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
  assert.doesNotMatch(workflow, /^  (?:miri|heavy)-runner:$/m, "selectors must not be jobs");
  const changes = indentedBlock(workflow, "changes", 2);
  assert.match(
    changes,
    /permissions:\n      contents: read\n      pull-requests: read\n    outputs:/,
  );
  assert.doesNotMatch(changes, /secrets\.|id-token:|actions: write|contents: write/);
  const checkout = changesStep(workflow, "Check out the runner selectors from main");
  assert.match(checkout, /ref: refs\/heads\/main/);
  assert.match(checkout, /path: \.ci-selectors/);
  assert.match(checkout, /persist-credentials: false/);
  assert.match(checkout, /continue-on-error: true/);
  const selector = changesStep(workflow, "Select Miri runner");
  assert.match(selector, /id: miri-runner/);
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
  assert.match(selector, /working-directory: \.ci-selectors/);
  assert.match(
    selector,
    /if \[ ! -f scripts\/select-miri-runner\.mjs \]; then[\s\S]*runner=ubuntu-26.04[\s\S]*else[\s\S]*node scripts\/select-miri-runner\.mjs/,
  );
  const miri = indentedBlock(workflow, "miri", 2);
  assert.doesNotMatch(miri, /continue-on-error/);
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
  const condition = stepCondition(changesStep(workflow, "Select Miri runner"));
  const checkoutCondition = stepCondition(
    changesStep(workflow, "Check out the runner selectors from main"),
  );
  assert.ok(condition, "selector must declare its eligibility condition");
  const context = {
    vars: { BLACKSMITH_MIRI_ALLOCATION: "", BLACKSMITH_HEAVY_ALLOCATION: "" },
    github: {
      repository: "fallow-rs/fallow",
      event_name: "push",
      ref: "refs/heads/main",
      run_attempt: 1,
      actor: "maintainer",
    },
    steps: { filter: { outputs: { miri: "true" } }, policy: { outputs: { "main-full": "true" } } },
    needs: { changes: { outputs: { miri: "true", "miri-runner": "ubuntu-26.04" } } },
    always: () => true,
    cancelled: () => false,
  };
  assert.equal(runInNewContext(condition, context), false, "default CI must not run a selector");
  assert.equal(runInNewContext(checkoutCondition, context), false, "nor check out main");
  context.vars.BLACKSMITH_MIRI_ALLOCATION = '{"month":"2026-09","firstRunNumber":100,"slots":2}';
  assert.equal(runInNewContext(condition, context), true, "an allocation must reach the selector");
  assert.equal(runInNewContext(checkoutCondition, context), true);
  const miri = indentedBlock(workflow, "miri", 2);
  assert.equal(runInNewContext(bracketKeys(miri.match(/^    if: (.+)$/m)[1]), context), true);
  assert.equal(
    runInNewContext(bracketKeys(miri.match(/^    runs-on: \$\{\{ (.+) \}\}$/m)[1]), context),
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
    "check-lint",
    "drift",
    "windows-rust",
    "windows-drift",
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
    if (["changes", "ci-ok"].includes(name)) continue;
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
    cache.split("- name: Build Miri test binaries")[0],
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
  const changes = indentedBlock(readWorkflow(".github/workflows/ci.yml"), "changes", 2);
  for (const id of ["miri-runner", "heavy-runner"]) {
    const output = changes.match(new RegExp(`^      ${id}: \\$\\{\\{ (.+) \\}\\}$`, "m"))?.[1];
    assert.ok(output, `Detect changes must publish ${id}`);
    const evaluate = (outcome) =>
      runInNewContext(bracketKeys(output), {
        steps: { [id]: { outcome, outputs: { runner: "blacksmith-4vcpu-ubuntu-2404" } } },
      });
    for (const outcome of ["failure", "skipped", "cancelled"]) {
      assert.equal(evaluate(outcome), "ubuntu-26.04", `${id} ${outcome}`);
    }
    assert.equal(evaluate("success"), "blacksmith-4vcpu-ubuntu-2404");
  }
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
    ["ci.yml", "check-lint"],
    ["release-validation.yml", "drift-full"],
  ]) {
    const workflow = readWorkflow(`.github/workflows/${file}`);
    const job = indentedBlock(workflow, jobName, 2);
    const runner = bracketKeys(
      job.match(/^    runs-on: (.+)$/m)[1].replace(/^\$\{\{ | \}\}$/g, ""),
    );
    const inCi = file === "ci.yml";
    const blacksmith = "blacksmith-4vcpu-ubuntu-2404";
    const context = {
      github: { run_attempt: 1, event_name: "push" },
      matrix: { partition: 1 },
      needs: {
        changes: {
          outputs: { rust: "true", "main-full": "true", "heavy-runner": blacksmith },
        },
        "heavy-runner": { result: "success", outputs: { runner: blacksmith } },
      },
      always: () => true,
      cancelled: () => false,
    };
    const setSelection = (runnerLabel, result = "success") => {
      context.needs.changes.outputs["heavy-runner"] = runnerLabel;
      context.needs["heavy-runner"] = { result, outputs: { runner: runnerLabel } };
    };
    const evaluateRunner = () =>
      runner === "ubuntu-26.04" ? runner : runInNewContext(runner, context);
    assert.equal(evaluateRunner(), blacksmith, `${file} must admit a reserved first attempt`);
    const condition = bracketKeys(job.match(/^    if: (.+)$/m)?.[1] ?? "");
    assert.ok(condition, `${file} must survive optional selector failure`);
    if (inCi) {
      assert.match(job, /^    needs: changes$/m, `${jobName} must not wait for a selector job`);
      setSelection("ubuntu-26.04");
      assert.equal(runInNewContext(condition, context), true);
      assert.equal(evaluateRunner(), "ubuntu-26.04", "the selector fallback output");
    } else {
      for (const result of ["failure", "skipped", "cancelled"]) {
        setSelection(blacksmith, result);
        assert.equal(runInNewContext(condition, context), true);
        assert.equal(evaluateRunner(), "ubuntu-26.04");
      }
    }
    setSelection(blacksmith);
    if (jobName === "check") {
      context.matrix.partition = 2;
      assert.equal(evaluateRunner(), "ubuntu-26.04", "the credit model reserves one Check job");
      context.matrix.partition = 1;
    }
    context.github.run_attempt = 2;
    assert.equal(evaluateRunner(), "ubuntu-26.04", "retained outputs cannot admit reruns");
    context.github.run_attempt = 1;
    for (const label of [undefined, "blacksmith-8vcpu-ubuntu-2404"]) {
      setSelection(label);
      assert.equal(evaluateRunner(), "ubuntu-26.04");
    }
    context.cancelled = () => true;
    assert.equal(runInNewContext(condition, context), false);
    if (inCi) {
      context.cancelled = () => false;
      context.github.event_name = "pull_request";
      context.needs.changes.outputs.rust = "false";
      assert.equal(runInNewContext(condition, context), false, "Check retains its path condition");
    }
  }
});

const heavySelectorContext = (eventName) => ({
  vars: { BLACKSMITH_HEAVY_ALLOCATION: "", BLACKSMITH_MIRI_ALLOCATION: "" },
  github: {
    repository: "fallow-rs/fallow",
    run_attempt: 1,
    actor: "maintainer",
    ref: "refs/heads/main",
    event_name: eventName,
    event: { pull_request: { head: { repo: { full_name: "fallow-rs/fallow" } } } },
  },
  steps: { filter: { outputs: { rust: "true" } }, policy: { outputs: { "main-full": "true" } } },
  needs: { changes: { outputs: { rust: "true", "main-full": "true" } } },
});

// Shared eligibility: no variable, reruns, Dependabot and forks never select.
const assertHeavySelectorEligibility = (runInNewContext, condition, context) => {
  assert.ok(condition, "the selector must gate startup");
  assert.equal(runInNewContext(condition, context), false, "no variable means no selector startup");
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
};

const assertHeavySelectorScript = (runInNewContext, selector, output) => {
  assert.match(
    selector,
    /BLACKSMITH_MIRI_ALLOCATION: \$\{\{ vars\.BLACKSMITH_MIRI_ALLOCATION \}\}/,
  );
  assert.match(selector, /if \[ ! -f scripts\/select-heavy-runner\.mjs \]; then/);
  assert.doesNotMatch(selector, /secrets\.|id-token:|contents: write/);
  assert.ok(output);
  for (const outcome of ["success", "failure", "skipped", "cancelled"]) {
    const step = { outcome, outputs: { runner: "blacksmith-4vcpu-ubuntu-2404" } };
    assert.equal(
      runInNewContext(bracketKeys(output), { steps: { runner: step, "heavy-runner": step } }),
      outcome === "success" ? "blacksmith-4vcpu-ubuntu-2404" : "ubuntu-26.04",
    );
  }
};

test("the CI heavy selector step skips startup when unset and publishes only trusted output", async () => {
  const { runInNewContext } = await import("node:vm");
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const selector = changesStep(workflow, "Select Check runner");
  const condition = stepCondition(selector);
  const context = heavySelectorContext("push");
  assertHeavySelectorEligibility(runInNewContext, condition, context);
  context.github.event_name = "pull_request";
  assert.equal(runInNewContext(condition, context), true);
  context.github.event.pull_request.head.repo.full_name = "fork/fallow";
  assert.equal(runInNewContext(condition, context), false);
  context.github.event_name = "push";
  context.steps.filter.outputs.rust = "false";
  context.steps.policy.outputs["main-full"] = "false";
  assert.equal(runInNewContext(condition, context), false);
  const checkout = changesStep(workflow, "Check out the runner selectors from main");
  assert.match(checkout, /ref: refs\/heads\/main\n          path: \.ci-selectors/);
  assert.match(checkout, /persist-credentials: false/);
  assert.match(selector, /working-directory: \.ci-selectors/);
  const output = indentedBlock(workflow, "changes", 2).match(
    /^      heavy-runner: \$\{\{ (.+) \}\}$/m,
  )?.[1];
  assertHeavySelectorScript(runInNewContext, selector, output);
});

test("the release validation heavy selector skips startup when unset", async () => {
  const { runInNewContext } = await import("node:vm");
  const workflow = readWorkflow(".github/workflows/release-validation.yml");
  const selector = indentedBlock(workflow, "heavy-runner", 2);
  const condition = bracketKeys(
    selector.match(/    if: >-\n([\s\S]+?)\n    runs-on:/)?.[1].trim() ?? "",
  );
  const context = heavySelectorContext("workflow_dispatch");
  assertHeavySelectorEligibility(runInNewContext, condition, context);
  context.github.event_name = "schedule";
  assert.equal(runInNewContext(condition, context), true);
  context.github.ref = "refs/heads/topic";
  assert.equal(runInNewContext(condition, context), false);
  assert.match(selector, /runs-on: ubuntu-26.04/);
  assert.match(selector, /permissions:\n      contents: read\n/);
  assert.match(selector, /ref: refs\/heads\/main\n          persist-credentials: false/);
  assert.match(selector, /node-version: '22'/);
  const output = selector.match(/^      runner: \$\{\{ (.+) \}\}$/m)?.[1];
  assertHeavySelectorScript(runInNewContext, selector, output);
});

test("CI aggregate ignores optional selector failure while real Check failure propagates", async () => {
  const { spawnSync } = await import("node:child_process");
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const aggregate = indentedBlock(workflow, "ci-ok", 2);
  const names = aggregate.match(/^    needs: \[([^\]]+)\]/m)[1].split(/,\s*/);
  assert.ok(names.includes("check"));
  assert.ok(names.includes("check-lint"));
  assert.ok(names.includes("windows-drift"));
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
    // A CI Check slot covers every job that runs on the runner it selects:
    // Check and Lint and contracts.
    const slotJobs = job === "check" ? ["check", "check-lint"] : [job];
    // Each job has its own runner start, checkout and cache overhead.
    const credits = slotJobs
      .map((name) =>
        Number(indentedBlock(source, name, 2).match(/^    timeout-minutes: (\d+)$/m)[1]),
      )
      .reduce((sum, minutes) => sum + minutes * 2 + 30, 0);
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
      budgetCredits: credits,
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

// Git 2.55 starts a detached `git repack -d` after `git commit` when two loose
// objects share `objects/17`. The repack removes empty object directories
// while the next `git add` of a test fixture writes into them, and the test
// fails with "unable to create temporary file". Fixture helpers set
// `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_NOSYSTEM`, so only command-scope config
// from the environment reaches every one of them.
const TEST_COMMAND =
  /cargo (?:test|nextest|llvm-cov)|node --test|npm (?:run )?test\b|npm run verify|vitest|scripts\/test-/;

const GIT_MAINTENANCE_OFF = [
  "GIT_CONFIG_COUNT: '2'",
  "GIT_CONFIG_KEY_0: maintenance.auto",
  "GIT_CONFIG_VALUE_0: 'false'",
  "GIT_CONFIG_KEY_1: gc.auto",
  "GIT_CONFIG_VALUE_1: '0'",
];

test("every workflow that runs tests turns off git automatic maintenance", () => {
  const testWorkflows = readdirSync(".github/workflows")
    .filter((name) => /\.ya?ml$/.test(name))
    .filter((name) => TEST_COMMAND.test(readWorkflow(join(".github/workflows", name))));

  assert.notEqual(testWorkflows.length, 0, "the test-command pattern must match a workflow");
  for (const name of testWorkflows) {
    const env = indentedBlock(readWorkflow(join(".github/workflows", name)), "env", 0);
    for (const entry of GIT_MAINTENANCE_OFF) {
      assert.ok(
        env.split("\n").some((line) => line.trim() === entry),
        `${name} runs tests, so its top-level env must set ${entry}`,
      );
    }
  }
});

// A job-level or step-level `GIT_CONFIG_COUNT` replaces the top-level one, and
// the jobs below it then run without the maintenance setting.
test("only the top-level env of a workflow sets GIT_CONFIG variables", () => {
  for (const name of readdirSync(".github/workflows").filter((file) => /\.ya?ml$/.test(file))) {
    const source = readWorkflow(join(".github/workflows", name));
    const nested = source
      .split("\n")
      .filter((line) => /^\s{4,}GIT_CONFIG_(?:COUNT|KEY_\d+|VALUE_\d+):/.test(line));
    assert.deepEqual(nested, [], `${name} sets GIT_CONFIG variables below the top-level env`);
  }
});

test("Miri runs the three crates in parallel with the same test selection", () => {
  const miri = indentedBlock(readWorkflow(".github/workflows/ci.yml"), "miri", 2);
  assert.match(miri, /^    name: Miri$/m, "Miri is a required status check name");
  // `cargo miri nextest run` starts one Miri process for each test, and each
  // start costs seconds, so it is slower than one `cargo miri test` per crate.
  assert.doesNotMatch(miri, /^\s+cargo [^\n]*miri nextest/m);
  const toolchain = 'cargo +"$MIRI_TOOLCHAIN" miri test';
  for (const command of [
    `${toolchain} -p fallow-types --lib --tests -- --skip proptests`,
    `${toolchain} -p fallow-graph --lib --tests -- "\${graph_shard_1[@]}" --skip proptests`,
    `${toolchain} -p fallow-graph --lib --tests -- "\${graph_shard_2[@]}" --skip proptests`,
    `${toolchain} -p fallow-graph --lib --tests -- "\${graph_shard_3[@]}" --skip proptests`,
    `${toolchain} -p fallow-extract --lib --tests css:: -- --skip sfc_css::`,
    `${toolchain} -p fallow-extract --lib --tests css_classes::`,
    `${toolchain} -p fallow-extract --lib --tests css_metrics::`,
    `${toolchain} -p fallow-extract --lib --tests suppress::`,
    `${toolchain} -p fallow-extract --lib --tests visitor::helpers::`,
  ]) {
    assert.equal(miri.split(`${command} || status=1`).length, 2, `Miri must run ${command} once`);
  }
  for (const crate of ["fallow-types", "fallow-graph", "fallow-extract"]) {
    assert.ok(miri.includes(`${toolchain} -p ${crate} --lib --tests --no-run`));
  }
  // Shard 3 skips every filter of shards 1 and 2, so each fallow-graph test
  // runs in exactly one shard and a new module cannot fall out of the run.
  assert.ok(
    miri.includes(
      'for filter in "${graph_shard_1[@]}" "${graph_shard_2[@]}"; do graph_shard_3+=(--skip "$filter"); done',
    ),
    "the last fallow-graph shard must skip exactly the filters of the other shards",
  );
  assert.match(miri, /^ {10}graph_shard_3=\(\)$/m);
  // A filter matches a substring, so a filter inside another filter runs the
  // same tests in shards 1 and 2.
  const filters = ["graph_shard_1", "graph_shard_2"].flatMap((name) =>
    miri.match(new RegExp(`^ {10}${name}=\\(([^)]+)\\)$`, "m"))[1].split(" "),
  );
  for (const filter of filters) {
    const others = filters.filter((other) => other !== filter && other.includes(filter));
    assert.deepEqual(others, [], `fallow-graph shard filter ${filter} overlaps ${others}`);
  }
  assert.ok(
    miri.includes(
      "crates=(fallow-types fallow-graph-1 fallow-graph-2 fallow-graph-3 fallow-extract)",
    ),
    "Miri must start every unit",
  );
  const extract = miri.slice(
    miri.indexOf("fallow-extract)"),
    miri.indexOf(";;", miri.indexOf("fallow-extract)")),
  );
  assert.match(extract, /export MIRIFLAGS=-Zmiri-tree-borrows/);
  assert.equal(miri.match(/MIRIFLAGS/g)?.length, 1, "types and graph stay on Stacked Borrows");
  // Each crate runs in the background and streams its output live, so a run
  // that hits the job timeout still shows the hung test. The step fails when
  // one crate fails or does not write its exit status.
  assert.ok(
    miri.includes(
      '{ run_crate "$crate" 2>&1 && s=0 || s=$?; echo "$s" > "$logs/$crate.status"; } \\\n' +
        '              | tee "$logs/$crate.log" | sed -u "s/^/[$crate] /" &',
    ),
    "Miri must stream each crate live and record its exit status",
  );
  assert.ok(miri.includes('for pid in "${pids[@]}"; do wait "$pid" || true; done'));
  assert.ok(
    miri.includes(
      'if [ "$(cat "$logs/$crate.status" 2>/dev/null)" = 0 ]; then result=passed; else result=failed; failed=1; fi',
    ),
    "Miri must fail a crate that has no zero exit status",
  );
  assert.match(miri, /exit "\$failed"/);
});

test("the Check, Lint and contracts, and Windows jobs run each step exactly once", () => {
  const workflow = readWorkflow(".github/workflows/ci.yml");
  const jobs = {
    check: [
      "Run tests",
      "Upload test report",
      "Run coverage producer conformance",
      "Run runtime-coverage integration tests (feature-gated stub sidecar)",
      "Run schema drift gate (feature-gated schema-emit binary)",
    ],
    "check-lint": [
      "Install contract bundle dependencies",
      "Run staged subgenerator integration tests",
      "Run contract bundle drift gate",
      "Clippy",
      "Clippy (test-sidecar-key and schema-emit features)",
      "Format",
    ],
    napi: ["Install NAPI package dependencies", "Build NAPI package", "Run NAPI smoke test"],
    "windows-rust": ["Test Windows path and subprocess handling", "Clippy platform-specific paths"],
    "windows-drift": ["Build fallow-mcp for the drift harness", "Test drift harness on Windows"],
  };
  const stepNames = (text) => [...text.matchAll(/^ {6}- name: (.+)$/gm)].map((match) => match[1]);
  const allSteps = Object.keys(jobs).flatMap((job) => stepNames(indentedBlock(workflow, job, 2)));
  for (const [job, steps] of Object.entries(jobs)) {
    const jobSteps = stepNames(indentedBlock(workflow, job, 2));
    for (const step of steps) {
      assert.ok(jobSteps.includes(step), `${job} must run "${step}"`);
      if (step === "Upload test report") continue;
      assert.equal(
        allSteps.filter((name) => name === step).length,
        1,
        `"${step}" must run in exactly one of these jobs`,
      );
    }
  }
  assert.equal(
    indentedBlock(workflow, "check", 2).match(/^    name: (.+)$/m)?.[1],
    "Check (${{ matrix.partition }}/2)",
  );
  assert.equal(
    indentedBlock(workflow, "windows-rust", 2).match(/^    name: (.+)$/m)?.[1],
    "Windows path and subprocess handling",
  );
  const check = indentedBlock(workflow, "check", 2);
  const lint = indentedBlock(workflow, "check-lint", 2);
  assert.doesNotMatch(check, /cargo clippy|cargo fmt|napi build/);
  assert.doesNotMatch(lint, /cargo nextest run|cargo test /);
  const napi = indentedBlock(workflow, "napi", 2);
  assert.doesNotMatch(lint, /napi build/);
  assert.match(check, /--partition hash:\$\{\{ matrix\.partition \}\}\/2/);
  assert.match(check, /partition: \[1, 2\]/);
  for (const job of [check, lint, napi]) {
    assert.match(job, /^    needs: changes$/m);
    assert.equal(job.match(/^    if: (.+)$/m)?.[1], check.match(/^    if: (.+)$/m)?.[1]);
  }
  const lintRunner = lint.match(/^    runs-on: \$\{\{ (.+) \}\}$/m)?.[1];
  assert.equal(
    check.match(/^    runs-on: (.+)$/m)?.[1],
    `\${{ matrix.partition == 1 && ${lintRunner} }}`,
    "only Check partition 1 shares the Lint and contracts runner",
  );
  assert.match(
    napi,
    /^    runs-on: ubuntu-26\.04$/m,
    "NAPI is outside the Blacksmith credit model",
  );
});

test("release build dependencies never receive the binary signing credential", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const build = indentedBlock(workflow, "build", 2);
  assert.doesNotMatch(
    build,
    /secrets\.|environment: release/u,
    "dependency execution must run outside the signing environment",
  );
  const signing = indentedBlock(workflow, "sign", 2);
  assert.match(signing, /needs: \[build, authenticode\]/u);
  assert.match(signing, /environment: release/u);
  assert.doesNotMatch(
    signing,
    /npm (?:ci|install)|pnpm install|cargo |pip install|cache@/u,
    "signing runner must not build or install repository dependencies",
  );
  assert.match(build, /name: unsigned-binaries-\$\{\{ matrix\.npm_dir \}\}/u);
  assert.doesNotMatch(build, /name: npm-\$\{\{|name: fallow-cli-\$\{\{/u);
  const buildPlatforms = Array.from(build.matchAll(/^\s+npm_dir: (\S+)$/gmu), (match) => match[1]);
  const signingPlatforms = Array.from(
    signing.matchAll(/^\s+- npm_dir: (\S+)$/gmu),
    (match) => match[1],
  );
  assert.deepEqual(
    signingPlatforms.toSorted(),
    buildPlatforms.toSorted(),
    "every built platform must be signed exactly once",
  );
  for (const consumer of ["npm-prep", "similar-code-conformance"]) {
    assert.match(
      indentedBlock(workflow, consumer, 2),
      /needs: \[build, sign\]/u,
      `${consumer} must wait for signed artifacts`,
    );
  }
});

test("Windows binaries get an Authenticode signature before packaging", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const build = indentedBlock(workflow, "build", 2);
  const authenticode = indentedBlock(workflow, "authenticode", 2);
  assert.match(authenticode, /needs: build/u);
  assert.match(authenticode, /^ {4}environment: windows-signing$/mu);
  assert.equal(
    workflow.match(/environment: windows-signing/gu)?.length,
    1,
    "only the Authenticode job may request the signing identity",
  );
  assert.match(authenticode, /permissions:\n\s+contents: read\n\s+id-token: write\n/u);
  assert.doesNotMatch(
    authenticode,
    /secrets\.|actions\/checkout|npm (?:ci|install)|cargo |cache@/u,
    "the Authenticode runner must not read secrets or run repository code",
  );
  assert.match(authenticode, /uses: Azure\/artifact-signing-action@[0-9a-f]{40} #/u);
  assert.match(authenticode, /case "\$\{WINDOWS_SIGNING_MODE:-off\}" in/u);

  const windowsBuilds = Array.from(
    build.matchAll(/- target: (\S+-pc-windows-msvc)\n(?:\s+\w+: .+\n)*?\s+npm_dir: (\S+)/gu),
    (match) => `${match[2]} ${match[1]}`,
  );
  const signedBuilds = Array.from(
    authenticode.matchAll(/- npm_dir: (\S+)\n\s+target: (\S+)/gu),
    (match) => `${match[1]} ${match[2]}`,
  );
  assert.deepEqual(signedBuilds.toSorted(), windowsBuilds.toSorted());

  for (const name of [
    "fallow",
    "fallow-multicall",
    "fallow-lsp",
    "fallow-mcp",
    "fallow-similar-code",
  ]) {
    assert.match(
      authenticode,
      new RegExp(String.raw`\\payload\\${name}-\$\{\{ matrix\.npm_dir \}\}\.exe`, "u"),
    );
  }
  assert.match(authenticode, /\\napi\\fallow-node\.\$\{\{ matrix\.npm_dir \}\}\.node/u);
  for (const artifact of [
    "unsigned-binaries-${{ matrix.npm_dir }}",
    "napi-${{ matrix.npm_dir }}",
    "fallow-${{ matrix.target }}",
  ]) {
    assert.ok(
      authenticode.includes(`name: ${artifact}\n`),
      `${artifact} must be replaced by its signed copy`,
    );
  }
  assert.equal(authenticode.match(/overwrite: true/gu)?.length, 3);
});

test("npm publishing uses only environment-scoped trusted publishing", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const publisher = indentedBlock(workflow, "npm-publish", 2);
  assert.doesNotMatch(
    publisher,
    /secrets\.NPM_TOKEN/u,
    "a stolen bootstrap token must not bypass the trusted publisher policy",
  );
  assert.match(publisher, /NODE_AUTH_TOKEN: ''/u);
  assert.match(publisher, /id-token: write/u);
});

test("root npm installs disable lifecycle scripts by default", (t) => {
  assert.ok(existsSync(".npmrc"), "root npm installation must have a lifecycle script policy");
  const cwd = mkdtempSync(join(tmpdir(), "fallow-lifecycle-policy-"));
  t.after(() => rmSync(cwd, { recursive: true, force: true }));
  writeFileSync(
    join(cwd, "package.json"),
    JSON.stringify({
      name: "lifecycle-policy-fixture",
      version: "1.0.0",
      scripts: { preinstall: "node -e \"require('fs').writeFileSync('EXECUTED', 'yes')\"" },
    }),
  );
  const run = (args) => runProcess("npm", args, { cwd, encoding: "utf8" });
  const lock = run([
    "install",
    "--package-lock-only",
    "--ignore-scripts",
    "--no-audit",
    "--no-fund",
  ]);
  assert.equal(lock.status, 0, lock.stderr);
  const unsafe = run(["ci", "--ignore-scripts=false", "--no-audit", "--no-fund"]);
  assert.equal(unsafe.status, 0, unsafe.stderr);
  assert.equal(readFileSync(join(cwd, "EXECUTED"), "utf8"), "yes");
  rmSync(join(cwd, "EXECUTED"));
  copyFileSync(".npmrc", join(cwd, ".npmrc"));
  const safe = run(["ci", "--no-audit", "--no-fund"]);
  assert.equal(safe.status, 0, safe.stderr);
  assert.equal(
    existsSync(join(cwd, "EXECUTED")),
    false,
    "the root npm policy must stop lifecycle code from running during ordinary installs",
  );
});

test("ordinary workflow npm installs suppress dependency lifecycle scripts", () => {
  for (const path of ["bench-type-aware.yml", "ci.yml"]) {
    const workflow = readWorkflow(`.github/workflows/${path}`);
    const installs = workflow
      .split("\n")
      .filter((line) => !line.trimStart().startsWith("#") && /\bnpm ci\b/u.test(line));
    assert.ok(installs.length > 0, `${path} must exercise the npm install policy`);
    for (const line of installs) {
      assert.ok(
        line.includes("--ignore-scripts"),
        `${path} permits dependency lifecycle scripts: ${line.trim()}`,
      );
    }
  }
});

test("VSIX publishers use reviewed frozen graphs from the validated source commit", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const extension = JSON.parse(readFileSync("editors/vscode/package.json", "utf8"));
  for (const [jobName, directory, cli, token] of [
    ["vscode-publish-marketplace", "marketplace", "@vscode/vsce", "VSCE_PAT"],
    ["vscode-publish-open-vsx", "open-vsx", "ovsx", "OVSX_PAT"],
  ]) {
    const job = indentedBlock(workflow, jobName, 2);
    assert.doesNotMatch(job, /npm install -g/u, "global installs resolve an unfrozen graph");
    const prefix = `.github/publishers/${directory}`;
    assert.ok(job.includes(`npm ci --prefix ${prefix} --ignore-scripts --no-audit --no-fund`));
    assert.ok(
      job.includes(
        `npx --no-install --prefix ${prefix} ${cli === "ovsx" ? "ovsx" : "vsce"} publish`,
      ),
    );
    assert.match(job, /ref: \$\{\{ github\.sha \}\}/u);
    assert.match(job, /persist-credentials: false/u);
    assert.ok(job.includes(`sparse-checkout: ${prefix}`));
    assert.match(job, /sparse-checkout-cone-mode: false/u);
    assert.doesNotMatch(job, /cache:|actions\/cache|npm install|pnpm|cargo /u);
    assert.match(job, /permissions:\n\s+contents: read/u);
    assert.equal(job.match(/secrets\./gu)?.length, 1);
    const authMode = cli === "ovsx" ? "OVSX_AUTH_MODE" : "VSCE_AUTH_MODE";
    assert.ok(job.includes(`${authMode}: \${{ vars.${authMode} || 'pat' }}`));
    const tokenCondition =
      cli === "ovsx"
        ? "vars.OVSX_AUTH_MODE != 'oidc'"
        : "(vars.VSCE_AUTH_MODE == '' || vars.VSCE_AUTH_MODE == 'pat')";
    assert.ok(job.includes(`${token}: \${{ ${tokenCondition} && secrets.${token} || '' }}`));
    assert.match(job, /id-token: write/u);

    const manifest = JSON.parse(readFileSync(`${prefix}/package.json`, "utf8"));
    const lock = JSON.parse(readFileSync(`${prefix}/package-lock.json`, "utf8"));
    assert.equal(manifest.private, true);
    assert.equal(manifest.dependencies[cli], extension.devDependencies[cli]);
    assert.deepEqual(Object.keys(manifest.dependencies), [cli]);
    assert.equal(lock.lockfileVersion, 3);
    assert.deepEqual(lock.packages[""].dependencies, manifest.dependencies);
    assert.equal(lock.packages[`node_modules/${cli}`].version, manifest.dependencies[cli]);
    if (cli === "ovsx") {
      assert.equal(manifest.overrides["@vscode/vsce"], extension.devDependencies["@vscode/vsce"]);
      assert.equal(
        lock.packages["node_modules/@vscode/vsce"].version,
        manifest.overrides["@vscode/vsce"],
      );
    }
    const dependencies = Object.entries(lock.packages).filter(([path]) => path !== "");
    assert.ok(dependencies.length > 0);
    for (const [path, dependency] of dependencies) {
      assert.match(path, /^node_modules\//u);
      assert.match(dependency.version, /^\d+\.\d+\.\d+(?:-[\w.-]+)?$/u, path);
      assert.match(dependency.resolved, /^https:\/\/registry\.npmjs\.org\//u, path);
      assert.match(dependency.integrity, /^sha512-[A-Za-z0-9+/]+={0,2}$/u, path);
      assert.equal(dependency.link, undefined, path);
    }
  }
});

test("Marketplace Entra login is conditional, subscription-free and follows frozen tool installation", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  const job = indentedBlock(workflow, "vscode-publish-marketplace", 2);
  const install = job.indexOf("- name: Install VS Code Marketplace publisher");
  const config = job.indexOf("- name: Validate Marketplace Azure identity configuration");
  const login = job.indexOf("- name: Authenticate Marketplace with Microsoft Entra");
  const publish = job.indexOf("- name: Publish every target to VS Code Marketplace");
  assert.ok(install >= 0 && install < config && config < login && login < publish);
  const configStep = job.slice(config, login);
  const loginStep = job.slice(login, publish);
  for (const step of [configStep, loginStep]) {
    assert.match(step, /if: vars\.VSCE_AUTH_MODE == 'azure'/u);
    assert.doesNotMatch(step, /secrets\./u);
  }
  assert.match(loginStep, /uses: azure\/login@[0-9a-f]{40}/u);
  assert.match(loginStep, /client-id: \$\{\{ vars\.VSCE_AZURE_CLIENT_ID \}\}/u);
  assert.match(loginStep, /tenant-id: \$\{\{ vars\.VSCE_AZURE_TENANT_ID \}\}/u);
  assert.match(loginStep, /allow-no-subscriptions: true/u);
  assert.match(loginStep, /audience: api:\/\/AzureADTokenExchange/u);
  assert.doesNotMatch(loginStep, /subscription-id:|creds:|continue-on-error:/u);
  assert.match(
    job.slice(publish),
    /MARKETPLACE_AZURE_AUTHENTICATED: \$\{\{ steps\.marketplace_azure_login\.outcome == 'success' \}\}/u,
  );
  assert.match(job, /environment: release/u);
  assert.match(job, /id-token: write/u);
});

test("Marketplace identity verification is a protected manual read with no publication credentials", () => {
  const workflow = readWorkflow(".github/workflows/verify-marketplace-identity.yml");
  const job = indentedBlock(workflow, "profile", 2);
  assert.match(workflow, /on:\n\s+workflow_dispatch:/u);
  assert.doesNotMatch(
    workflow,
    /^\s+(?:push|pull_request|pull_request_target|schedule|workflow_call):/mu,
  );
  assert.match(
    job,
    /if: github\.event_name == 'workflow_dispatch' && github\.ref == 'refs\/heads\/main'/u,
  );
  assert.match(job, /environment: release/u);
  assert.match(job, /permissions:\n\s+contents: read\n\s+id-token: write/u);
  assert.match(job, /ref: \$\{\{ github\.sha \}\}/u);
  assert.match(job, /persist-credentials: false/u);
  assert.match(job, /sparse-checkout: \|\n\s+\.github\/scripts\/marketplace-profile\.mjs/u);
  assert.match(job, /sparse-checkout-cone-mode: false/u);
  assert.match(job, /package-manager-cache: false/u);
  assert.match(job, /uses: azure\/login@[0-9a-f]{40}/u);
  assert.match(job, /client-id: \$\{\{ vars\.VSCE_AZURE_CLIENT_ID \}\}/u);
  assert.match(job, /tenant-id: \$\{\{ vars\.VSCE_AZURE_TENANT_ID \}\}/u);
  assert.match(job, /allow-no-subscriptions: true/u);
  assert.match(job, /audience: api:\/\/AzureADTokenExchange/u);
  assert.doesNotMatch(
    job,
    /secrets\.|VSCE_PAT|SIGNING|subscription-id:|creds:|continue-on-error:|npx|\bpublish\b|upload-artifact|contents: write/u,
  );
  const config = job.indexOf("--validate-config");
  const login = job.indexOf("uses: azure/login@");
  const profile = job.indexOf("- name: Read validated Marketplace profile ID");
  assert.ok(config >= 0 && config < login && login < profile);
});

test("release preflight checks secrets for the active publisher modes", () => {
  const procedure = readFileSync("docs/development/release-procedure.md", "utf8");
  const block = procedure.match(/```bash\n(   set -euo pipefail\n   custom=[\s\S]*?)   ```/u)?.[1];
  assert.ok(block, "release credential preflight shell block must exist");
  const run = (
    vsce,
    ovsx,
    repoSecrets,
    envSecrets,
    client = "18767c00-8be6-4496-8b73-e2cccd03a87a",
    windowsSigningMode = "authenticode",
  ) => {
    const shell = `gh() {
      case "$*" in
        *custom_branch_policies*) echo true ;;
        *deployment-branch-policies*) echo main ;;
        *"variable list"*"--env windows-signing"*) printf '%s' "$TEST_SIGNING_VARIABLES" ;;
        *"variable list"*) printf '%s' "$TEST_VARIABLES" ;;
        *"secret list"*"--env release"*) printf '%s' "$TEST_ENV_SECRETS" ;;
        *"secret list"*) printf '%s' "$TEST_REPO_SECRETS" ;;
        *) return 2 ;;
      esac
    }
${block}`;
    return runProcess("bash", ["-eu", "-c", shell], {
      encoding: "utf8",
      env: {
        ...process.env,
        TEST_VARIABLES: JSON.stringify([
          { name: "VSCE_AUTH_MODE", value: vsce },
          { name: "OVSX_AUTH_MODE", value: ovsx },
          { name: "VSCE_AZURE_CLIENT_ID", value: client },
          { name: "VSCE_AZURE_TENANT_ID", value: "3e942fa4-e6cf-44a2-b0ef-dc2b0c6c3761" },
        ]),
        TEST_SIGNING_VARIABLES: JSON.stringify([
          { name: "WINDOWS_SIGNING_MODE", value: windowsSigningMode },
        ]),
        TEST_REPO_SECRETS: repoSecrets.join("\n"),
        TEST_ENV_SECRETS: envSecrets.join("\n"),
      },
    });
  };
  const signing = "ED25519_BINARY_SIGNING_PRIVATE_KEY";
  const azure = run("azure", "oidc", [], [signing]);
  assert.equal(azure.status, 0, azure.stderr);
  const client = "18767c00-8be6-4496-8b73-e2cccd03a87a";
  assert.equal(run("azure", "oidc", [], [signing], client, "off").status, 0);
  assert.notEqual(run("azure", "oidc", [], [signing], client, "invalid").status, 0);
  assert.equal(run("oidc", "oidc", [], [signing]).status, 0);
  assert.notEqual(run("azure", "oidc", [], [signing], "invalid").status, 0);
  assert.notEqual(run("invalid", "oidc", [], [signing]).status, 0);
  assert.notEqual(run("azure", "invalid", [], [signing]).status, 0);
  const missing = run("", "", [], [signing]);
  assert.notEqual(missing.status, 0);
  assert.match(missing.stderr, /VSCE_PAT is missing/u);
  assert.equal(run("pat", "pat", [], [signing, "VSCE_PAT", "OVSX_PAT"]).status, 0);
  const retained = run("azure", "oidc", ["VSCE_PAT", "OVSX_PAT"], [signing]);
  assert.equal(retained.status, 0, retained.stderr);
  assert.match(retained.stderr, /VSCE_PAT is a repository secret/u);
  assert.match(retained.stderr, /OVSX_PAT is a repository secret/u);
  const duplicate = run("azure", "oidc", ["VSCE_PAT"], [signing, "VSCE_PAT"]);
  assert.notEqual(duplicate.status, 0);
  assert.match(duplicate.stderr, /VSCE_PAT exists at both levels/u);
  assert.notEqual(run("azure", "oidc", [], []).status, 0);
});

const NODE_SCRIPT_COMMAND = /\bnode\s+(?:\.\/)?([\w./-]+\.mjs)\b/gu;
// Each form gives the package directory in group 1 and the script name in
// group 2. A bare `pnpm <name>` can also be a pnpm command, so only the
// explicit forms must name a package script.
const PACKAGE_SCRIPT_FORMS = [
  { pattern: /\bnpm(?:\s+--prefix\s+([\w./-]+))?\s+run\s+([\w:-]+)/gu, explicit: true },
  { pattern: /\bnpm(?:\s+--prefix\s+([\w./-]+))?\s+(test)\b/gu, explicit: true },
  { pattern: /\bpnpm(?:\s+--dir\s+([\w./-]+))?\s+run\s+([\w:-]+)/gu, explicit: true },
  { pattern: /\bpnpm(?:\s+--dir\s+([\w./-]+))?\s+(?!run\b)([\w:-]+)/gu, explicit: false },
];
const PREFIX_OPTION = /\s--prefix\s+([\w./-]+)/u;
const CD_COMMAND = /(?:^|&&|;|\|\||\s)cd\s+([\w./-]+)\s*(?:&&|;|$)/u;
const JOB_KEY = /^ {2}[\w-]+:\s*$/u;
const STEP_START =
  /^\s*-\s+(?:name|uses|run|id|if|shell|env|with|working-directory|timeout-minutes|continue-on-error):/u;
const WORKING_DIRECTORY = /^\s*(?:-\s+)?working-directory:\s*["']?([\w./-]+)["']?\s*$/u;
const NODE_START_CALL =
  /\b(?:spawnSync|execFileSync|spawn|execFile)\s*\(\s*process\.execPath\s*,\s*\[([^\]]*)\]/gu;

const workflowSources = () => {
  const actionFiles = readdirSync(".github/actions", { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => join(".github/actions", entry.name, "action.yml"))
    .filter(existsSync);
  const workflowFiles = readdirSync(".github/workflows")
    .filter((name) => /\.ya?ml$/u.test(name))
    .map((name) => join(".github/workflows", name));
  return [...workflowFiles, ...actionFiles].map(readWorkflow);
};

const chunk = (lines, isStart) =>
  lines.reduce((chunks, line) => {
    if (chunks.length === 0 || isStart(line)) chunks.push([]);
    chunks.at(-1).push(line);
    return chunks;
  }, []);

const directoryOf = (lines) =>
  lines.map((line) => WORKING_DIRECTORY.exec(line)?.[1]).find((dir) => dir !== undefined);

// A step runs in its own `working-directory`, or else in the
// `defaults.run.working-directory` of its job.
const commandSteps = (source) =>
  chunk(source.split(/\r?\n/u), (line) => JOB_KEY.test(line)).flatMap((job) => {
    const [header, ...steps] = chunk(job, (line) => STEP_START.test(line));
    const jobDir = directoryOf(header) ?? ".";
    return steps.map((lines) => ({ lines, dir: directoryOf(lines) ?? jobDir }));
  });

const packageScripts = (packageDir) => {
  const manifest = join(packageDir, "package.json");
  return existsSync(manifest) ? (JSON.parse(readFileSync(manifest, "utf8")).scripts ?? {}) : {};
};

const packageCommands = (line, lineDir) =>
  PACKAGE_SCRIPT_FORMS.flatMap(({ pattern, explicit }) =>
    Array.from(line.matchAll(pattern), (match) => ({
      packageDir: match[1] ?? PREFIX_OPTION.exec(line.slice(match.index))?.[1] ?? lineDir,
      name: match[2],
      explicit,
    })),
  );

// A package script can run other package scripts, so follow each one once.
const followPackageScript = ({ packageDir, name, explicit }, state) => {
  const body = packageScripts(packageDir)[name];
  if (body === undefined) {
    if (explicit) state.unknown.push(`${packageDir}: ${name}`);
    return [];
  }
  const key = `${packageDir}:${name}`;
  if (state.seen.has(key)) return [];
  state.seen.add(key);
  return commandScripts(body.split("\n"), packageDir, state);
};

const lineScripts = (line, lineDir, state) => [
  ...Array.from(line.matchAll(NODE_SCRIPT_COMMAND), (match) => join(lineDir, match[1])),
  ...packageCommands(line, lineDir).flatMap((command) => followPackageScript(command, state)),
];

// A shell keeps the directory of a `cd`, so a `cd <dir>` moves every later
// command of the same step or package script.
const commandScripts = (lines, startDir, state) => {
  let dir = startDir;
  return lines.flatMap((line) => {
    const cd = CD_COMMAND.exec(line);
    if (cd) dir = join(dir, cd[1]);
    return lineScripts(line, dir, state);
  });
};

const workflowRunScripts = () => {
  const state = { seen: new Set(), unknown: [] };
  const scripts = workflowSources().flatMap((source) =>
    commandSteps(source).flatMap((step) => commandScripts(step.lines, step.dir, state)),
  );
  // CI runs the runner selectors from a checkout of main in `.ci-selectors`,
  // which holds the same tracked scripts.
  for (const [index, script] of scripts.entries()) {
    scripts[index] = script.replace(/^\.ci-selectors\//u, "");
  }
  return {
    scripts: [...new Set(scripts)]
      .filter((script) => !script.endsWith(".test.mjs") && !script.includes("node_modules"))
      .toSorted(),
    unknown: [...new Set(state.unknown)].toSorted(),
  };
};

// A main guard decides at run time if the module was started or imported.
// When it decides wrong, the script does no work and exits with 0.
const hasMainGuard = (source) =>
  source.includes("process.argv[1]") && source.includes("import.meta.url");

const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");

// A start test starts the script itself: the argument list of a
// `process.execPath` start call names the script, or names a constant whose
// value names it. A spawn of another file plus a mention elsewhere is no proof.
const hasStartTest = (script) => {
  const testPath = script.replace(/\.mjs$/u, ".test.mjs");
  if (!existsSync(testPath)) return false;
  const source = readFileSync(testPath, "utf8");
  const name = escapeRegExp(script.slice(script.lastIndexOf("/") + 1));
  const namesScript = (text) => new RegExp(`["'\`/]${name}["'\`]`, "u").test(text);
  const constantNamesScript = (identifier) =>
    new RegExp(`\\b(?:const|let)\\s+${identifier}\\s*=[^;]*["'\`/]${name}["'\`]`, "u").test(source);
  return Array.from(source.matchAll(NODE_START_CALL), (match) => match[1]).some(
    (args) =>
      namesScript(args) ||
      Array.from(args.matchAll(/(?<![\w$.])[A-Za-z_$][\w$]*/gu), (id) => id[0]).some(
        constantNamesScript,
      ),
  );
};

test("every guarded script that a workflow runs has a start test", () => {
  const { scripts, unknown } = workflowRunScripts();
  assert.deepEqual(unknown, [], "a workflow runs a package script that does not exist");
  assert.ok(scripts.length > 0, "found no workflow script commands");
  for (const script of scripts) {
    assert.ok(existsSync(script), `${script} is run by a workflow but does not exist`);
  }
  const missing = scripts.filter(
    (script) => hasMainGuard(readFileSync(script, "utf8")) && !hasStartTest(script),
  );
  assert.deepEqual(
    missing,
    [],
    "A workflow runs these scripts, and each has a main guard but no start test. " +
      "Add a sibling <name>.test.mjs that starts the script as a process and " +
      "checks that its work began, so a guard that never matches cannot pass CI.",
  );
});

// A file URL built from a string skips URL encoding, so the guard never
// matches a path with a space or a `%`, and the script does no work.
const STRING_FILE_URL_GUARD = /`file:\/\/\$\{process\.argv\[1\]\}`/u;

test("no main guard builds a file URL from a string", () => {
  const listed = runProcess("git", ["ls-files", "*.mjs"], { encoding: "utf8" });
  assert.equal(listed.status, 0, listed.stderr);
  const offenders = listed.stdout
    .split("\n")
    .filter((path) => path !== "" && existsSync(path))
    .filter((path) => STRING_FILE_URL_GUARD.test(readFileSync(path, "utf8")));
  assert.deepEqual(
    offenders,
    [],
    "Use `pathToFileURL(process.argv[1]).href` or compare real paths in the main guard.",
  );
});

test("release auth check cannot start any release build or publication job", () => {
  const workflow = readWorkflow(".github/workflows/release.yml");
  assert.match(
    workflow,
    /auth_check:\n\s+description:[^\n]*\n\s+required: false\n\s+default: false\n\s+type: boolean/u,
  );
  const context = indentedBlock(workflow, "release-context", 2);
  assert.match(context, /^    if: \$\{\{ !inputs.auth_check \}\}$/mu);
  const names = Array.from(
    indentedBlock(workflow, "jobs", 0).matchAll(/^ {2}([a-z][a-z0-9-]*):$/gmu),
    (match) => match[1],
  );
  const reachable = new Set(["release-context"]);
  for (let pass = 0; pass < names.length; pass++) {
    for (const name of names) {
      if (name === "publisher-auth-check") continue;
      const job = indentedBlock(workflow, name, 2);
      assert.match(job, /^    if: \$\{\{ !inputs.auth_check \}\}$/mu, name);
      assert.doesNotMatch(job, /always\(\)|!cancelled\(\)/u);
      const needs =
        job
          .match(/^    needs: (.+)$/mu)?.[1]
          .replaceAll("[", "")
          .replaceAll("]", "")
          .split(/,\s*/u) ?? [];
      if (needs.length && needs.every((dependency) => reachable.has(dependency)))
        reachable.add(name);
    }
  }
  assert.deepEqual(
    [...reachable].toSorted(),
    names.filter((name) => name !== "publisher-auth-check").toSorted(),
  );
  const auth = indentedBlock(workflow, "publisher-auth-check", 2);
  assert.match(auth, /if: inputs.auth_check && github.ref == 'refs\/heads\/main'/u);
  assert.match(auth, /environment: release/u);
  assert.match(auth, /id-token: write/u);
  assert.doesNotMatch(
    auth,
    /needs:|secrets\.|npm|npx|cargo|upload-artifact|download-artifact|packagePath/u,
  );
  assert.match(auth, /node .github\/scripts\/publisher-auth-check.mjs/u);
});

test("auth-only npm exchanges use the exact release publication inventory", async () => {
  const { NPM_PACKAGES } = await import("../.github/scripts/publisher-auth-check.mjs");
  const workflow = readWorkflow(".github/workflows/release.yml");
  const publish = indentedBlock(workflow, "npm-publish", 2);
  const inventory = publish.match(/expected_names=\(([\s\S]*?)\)/u)?.[1];
  assert.ok(inventory);
  const expected = [...inventory.matchAll(/"([^"\n]+)"/gu)].map((match) => match[1]);
  assert.deepEqual(NPM_PACKAGES, expected);
  assert.equal(new Set(NPM_PACKAGES).size, NPM_PACKAGES.length);
});

test("optional Marketplace publisher proof installs only the frozen CLI before Azure login", () => {
  const workflow = readWorkflow(".github/workflows/verify-marketplace-identity.yml");
  const job = indentedBlock(workflow, "profile", 2);
  const install = job.match(
    /- name: Install frozen Marketplace verifier\n([\s\S]*?)(?=\n      -|$)/u,
  )?.[1];
  assert.ok(install, "publisher verification needs the frozen native CLI");
  assert.match(install, /if: inputs.verify_publisher/u);
  assert.match(
    install,
    /run: npm ci --prefix .github\/publishers\/marketplace --ignore-scripts --no-audit --no-fund/u,
  );
  assert.ok(job.indexOf("Install frozen Marketplace verifier") < job.indexOf("uses: azure/login@"));
  const native = job.match(
    /- name: Verify native Marketplace Azure credential chain\n([\s\S]*?)(?=\n      -|$)/u,
  )?.[1];
  assert.ok(native);
  assert.match(native, /if: inputs.verify_publisher/u);
  assert.match(native, /run: node .github\/scripts\/marketplace-profile.mjs --verify-vsce/u);
  assert.ok(job.indexOf("--verify-publisher") < job.indexOf("--verify-vsce"));
  assert.match(
    job,
    /sparse-checkout: \|\n\s+.github\/scripts\/marketplace-profile.mjs\n\s+.github\/publishers\/marketplace/u,
  );
});

// Each pull request job takes a runner slot from every other open pull
// request. A new workflow with a pull request trigger must pass the "Rule for
// new workflows" in docs/development/quality-gates.md before it joins this list.
const PULL_REQUEST_WORKFLOWS = [
  "bench-cli-instructions.yml",
  "bench-type-aware.yml",
  "bench.yml",
  "ci.yml",
  "codeql.yml",
  "commitlint.yml",
  "dependabot-auto-merge.yml",
  "ecosystem.yml",
  "pgo-validate.yml",
  "protocol-parity.yml",
  "signing-migration.yml",
  "test-action.yml",
];

test("only reviewed workflows run on pull requests", () => {
  const triggered = readdirSync(".github/workflows")
    .filter((file) => /\.ya?ml$/u.test(file))
    .filter((file) =>
      /^ {2}pull_request(?:_target)?:/mu.test(readWorkflow(`.github/workflows/${file}`)),
    )
    .toSorted();
  assert.deepEqual(
    triggered,
    PULL_REQUEST_WORKFLOWS,
    "A workflow with a pull_request trigger must be a required check, or catch a bug " +
      "that main cannot catch one commit later. Read the Rule for new workflows in " +
      "docs/development/quality-gates.md, then update PULL_REQUEST_WORKFLOWS.",
  );
});

test("the Dependabot caller pins the org-wide workflow and passes only what it needs", () => {
  const workflow = readWorkflow(".github/workflows/dependabot.yml");

  assert.match(workflow, /^    branches: \['dependabot\/\*\*'\]$/mu);
  assert.match(
    workflow,
    /^    uses: fallow-rs\/\.github\/\.github\/workflows\/dependabot\.yml@[0-9a-f]{40} # main$/mu,
  );
  assert.doesNotMatch(workflow, /secrets: inherit/u, "pass each secret by name");
  // The reusable workflow checks the source repository too, but the caller
  // must not hand secrets to a run that a fork started.
  assert.match(
    workflow,
    /github\.event\.workflow_run\.head_repository\.full_name == github\.repository/u,
  );
  assert.doesNotMatch(
    workflow,
    /^\s+(?:contents|id-token): write$/mu,
    "only the App token may push",
  );
});

test("release reads each platform package name from a path that Node resolves as a file", () => {
  const job = indentedBlock(readWorkflow(".github/workflows/release.yml"), "npm-prep", 2);
  const step = workflowSteps(job).find((text) =>
    text.startsWith("name: Bundle README and LICENSE into platform packages"),
  );
  assert.ok(step, "missing the platform package docs step");
  // The two node commands need the full repository. The package loop needs only
  // the package manifests and the LICENSE files.
  const loop = step
    .split("\n")
    .filter((line) => !line.includes("prepare-platform-package-docs.mjs"))
    .slice(2)
    .map((line) => line.replace(/^ {10}/u, ""))
    .join("\n");
  const root = mkdtempSync(join(tmpdir(), "fallow-platform-docs-"));
  try {
    writeFileSync(join(root, "LICENSE"), "MIT License\n");
    for (const [dir, name] of [
      ["npm/darwin-arm64", "@fallow-cli/darwin-arm64"],
      ["npm/fallow", "fallow"],
      ["crates/napi/npm/darwin-arm64", "@fallow-cli/fallow-node-darwin-arm64"],
      ["crates/napi", "@fallow-cli/fallow-node"],
    ]) {
      const path = join(root, dir);
      mkdirSync(path, { recursive: true });
      writeFileSync(join(path, "package.json"), JSON.stringify({ name }));
      if (name.startsWith("@fallow-cli/")) writeFileSync(join(path, "LICENSE"), "MIT License\n");
    }
    const result = runProcess("bash", ["-e", "-c", loop], {
      cwd: root,
      encoding: "utf8",
      env: { ...process.env, GITHUB_WORKSPACE: root },
    });
    assert.equal(result.status, 0, result.stderr);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
