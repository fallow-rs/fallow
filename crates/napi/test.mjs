import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { strict as assert } from "node:assert";
import { execFileSync, spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

import {
  computeComplexity,
  computeHealth,
  detectBoundaryViolations,
  detectCircularDependencies,
  detectDeadCode,
  detectDuplication,
  detectFeatureFlags,
  detectSimilarCode,
} from "./index.js";

const require = createRequire(import.meta.url);
const { typeAwareCommand } = require("./type-aware-command.js");

assert.equal(typeof detectSimilarCode, "function");

const napiRoot = dirname(fileURLToPath(import.meta.url));
const napiCliRoot = dirname(require.resolve("@napi-rs/cli/package.json"));
const typescriptRoot = dirname(
  require.resolve("typescript/package.json", { paths: [napiCliRoot] }),
);
execFileSync(
  process.execPath,
  [
    join(typescriptRoot, "bin", "tsc"),
    "--project",
    join(napiRoot, "tests", "types", "tsconfig.json"),
  ],
  {
    stdio: "pipe",
  },
);
console.log("  [PASS] similar-code declarations compile");

function makeFixture() {
  const root = mkdtempSync(join(tmpdir(), "fallow-node-"));
  mkdirSync(join(root, "src", "application"), { recursive: true });
  mkdirSync(join(root, "src", "domain"), { recursive: true });

  writeFileSync(
    join(root, "package.json"),
    JSON.stringify(
      {
        name: "fallow-node-fixture",
        version: "1.0.0",
        main: "src/main.ts",
      },
      null,
      2,
    ) + "\n",
  );

  writeFileSync(
    join(root, ".fallowrc.json"),
    JSON.stringify(
      {
        boundaries: {
          preset: "layered",
        },
      },
      null,
      2,
    ) + "\n",
  );

  writeFileSync(
    join(root, "src", "main.ts"),
    `
import { usedThing } from './application/service';
import './cycle-a';
import './domain/model';

export function run() {
  if (process.env.FEATURE_ALPHA) {
    console.log('flag on');
  }

  return usedThing();
}

run();
`.trimStart(),
  );

  writeFileSync(
    join(root, "src", "application", "service.ts"),
    `
export function usedThing() {
  return 'ok';
}

export const unusedThing = 42;

export function complexPath(input: number) {
  if (input > 10) {
    if (input % 2 === 0) {
      return 'a';
    }
    return 'b';
  }
  if (input > 5) {
    return 'c';
  }
  return 'd';
}
`.trimStart(),
  );

  writeFileSync(
    join(root, "src", "domain", "model.ts"),
    `
import { usedThing } from '../application/service';

export const domainValue = usedThing();
`.trimStart(),
  );

  writeFileSync(
    join(root, "src", "cycle-a.ts"),
    `
import { cycleB } from './cycle-b';

export const cycleA = cycleB + 1;
`.trimStart(),
  );

  writeFileSync(
    join(root, "src", "cycle-b.ts"),
    `
import { cycleA } from './cycle-a';

export const cycleB = cycleA + 1;
`.trimStart(),
  );

  writeFileSync(
    join(root, "src", "dup-one.ts"),
    `
export function duplicatedOne(items: number[]) {
  let total = 0;
  for (const item of items) {
    if (item > 10) {
      total += item * 2;
    } else if (item > 5) {
      total += item + 3;
    } else {
      total += item - 1;
    }
  }
  return total;
}
`.trimStart(),
  );

  writeFileSync(
    join(root, "src", "dup-two.ts"),
    `
export function duplicatedTwo(items: number[]) {
  let total = 0;
  for (const item of items) {
    if (item > 10) {
      total += item * 2;
    } else if (item > 5) {
      total += item + 3;
    } else {
      total += item - 1;
    }
  }
  return total;
}
`.trimStart(),
  );

  execFileSync("git", ["init"], { cwd: root, stdio: "ignore" });
  execFileSync("git", ["config", "user.name", "Fallow Node Test"], { cwd: root, stdio: "ignore" });
  execFileSync("git", ["config", "user.email", "fallow-node@example.com"], {
    cwd: root,
    stdio: "ignore",
  });
  execFileSync("git", ["config", "commit.gpgsign", "false"], { cwd: root, stdio: "ignore" });
  execFileSync("git", ["add", "."], { cwd: root, stdio: "ignore" });
  execFileSync("git", ["commit", "-m", "fixture"], { cwd: root, stdio: "ignore" });

  return root;
}

function makeAdversarialFixture() {
  const root = mkdtempSync(join(tmpdir(), "fallow-node-adversarial-"));
  mkdirSync(join(root, "src"), { recursive: true });
  writeFileSync(
    join(root, "package.json"),
    JSON.stringify(
      {
        name: "fallow-node-adversarial",
        version: "1.0.0",
        main: "src/main.ts",
      },
      null,
      2,
    ) + "\n",
  );
  writeFileSync(join(root, "src", "main.ts"), "export const ok = 1;\n");
  writeFileSync(join(root, "src", "broken.ts"), "export function nope( {\n");
  writeFileSync(join(root, "src", "invalid.ts"), Buffer.from([0xff, 0xfe, 0x00]));
  return root;
}

function runPanicBoundaryChild() {
  const script = String.raw`
const { mkdtempSync, mkdirSync, writeFileSync } = require("node:fs");
const { tmpdir } = require("node:os");
const { join } = require("node:path");
const { detectDeadCode } = require("./index.js");

const root = mkdtempSync(join(tmpdir(), "fallow-node-panic-"));
mkdirSync(join(root, "src"), { recursive: true });
writeFileSync(join(root, "package.json"), JSON.stringify({ name: "panic-fixture", main: "src/main.ts" }) + "\n");
writeFileSync(join(root, "src", "main.ts"), "export const value = 1;\n");

(async () => {
  try {
    await detectDeadCode({ root });
    throw new Error("expected FALLOW_NAPI_TEST_PANIC to reject");
  } catch (error) {
    if (error.name !== "FallowNodeError" || error.code !== "FALLOW_PANIC") {
      throw error;
    }
    console.log("CAUGHT:" + error.code + ":" + error.name);
  }
})().catch((error) => {
  console.error(error && error.stack ? error.stack : String(error));
  process.exit(1);
});
`;
  return spawnSync(process.execPath, ["-e", script], {
    cwd: process.cwd(),
    encoding: "utf8",
    env: { ...process.env, FALLOW_NAPI_TEST_PANIC: "1" },
  });
}

function runLoaderCompanionFixture(companionVersion) {
  const work = mkdtempSync(join(tmpdir(), "fallow-node-loader-"));
  const packageRoot = join(work, "node_modules", "fallow-type-aware");
  mkdirSync(packageRoot, { recursive: true });
  writeFileSync(
    join(work, "package.json"),
    JSON.stringify({ name: "@fallow-cli/fallow-node", version: "3.8.0" }),
  );
  writeFileSync(
    join(work, "index.js"),
    "module.exports = { binary: process.env.FALLOW_TYPE_AWARE_BIN || null, script: process.env.FALLOW_TYPE_AWARE_SCRIPT || null, source: process.env.FALLOW_TYPE_AWARE_BIN_SOURCE || null };\n",
  );
  writeFileSync(join(work, "loader.js"), readFileSync(join(process.cwd(), "loader.js"), "utf8"));
  writeFileSync(
    join(work, "type-aware-command.js"),
    readFileSync(join(process.cwd(), "type-aware-command.js"), "utf8"),
  );
  writeFileSync(
    join(packageRoot, "package.json"),
    JSON.stringify({ name: "fallow-type-aware", version: companionVersion }),
  );
  writeFileSync(join(packageRoot, "fallow-type-aware.mjs"), "#!/usr/bin/env node\n");
  const {
    FALLOW_TYPE_AWARE_BIN: _ignoredBinary,
    FALLOW_TYPE_AWARE_SCRIPT: _ignoredScript,
    FALLOW_TYPE_AWARE_BIN_SOURCE: _ignoredSource,
    ...env
  } = process.env;
  const child = spawnSync(
    process.execPath,
    ["-e", "process.stdout.write(JSON.stringify(require('./loader.js')))"],
    { cwd: work, encoding: "utf8", env },
  );
  rmSync(work, { recursive: true, force: true });
  return child;
}

function runSimilarCodeVerificationFailureFixture() {
  const work = mkdtempSync(join(tmpdir(), "fallow-node-similar-code-loader-"));
  const companionRoot = join(work, "node_modules", "fallow-similar-code");
  const scriptsRoot = join(companionRoot, "scripts");
  const packageVersion = JSON.parse(
    readFileSync(join(process.cwd(), "package.json"), "utf8"),
  ).version;
  mkdirSync(scriptsRoot, { recursive: true });
  writeFileSync(
    join(work, "package.json"),
    JSON.stringify({ name: "@fallow-cli/fallow-node", version: packageVersion }),
  );
  writeFileSync(
    join(work, "index.js"),
    "module.exports = { detectSimilarCode: () => ({ unexpected: true }) };\n",
  );
  writeFileSync(join(work, "loader.js"), readFileSync(join(process.cwd(), "loader.js"), "utf8"));
  writeFileSync(
    join(work, "type-aware-command.js"),
    readFileSync(join(process.cwd(), "type-aware-command.js"), "utf8"),
  );
  writeFileSync(
    join(companionRoot, "package.json"),
    JSON.stringify({ name: "fallow-similar-code", version: packageVersion }),
  );
  writeFileSync(
    join(scriptsRoot, "run-binary.js"),
    `module.exports = {
      resolvePlatformPackage: () => "@fallow-cli/fallow-similar-code-test",
      resolveBinaryArtifact: (packageName) => ({
        packageName,
        packageVersion: ${JSON.stringify(packageVersion)},
        manifestPath: "/native/package.json",
        binaryName: "fallow-similar-code",
        binaryPath: "/native/fallow-similar-code"
      })
    };\n`,
  );
  writeFileSync(
    join(scriptsRoot, "verify-binary.js"),
    `module.exports = {
      verifyBinary: () => ({ ok: false, code: "digest-missing", message: "missing digest" })
    };\n`,
  );
  const script = `
    try {
      require("./loader.js").detectSimilarCode({});
      process.exitCode = 2;
    } catch (error) {
      process.stdout.write(JSON.stringify({
        name: error.name,
        code: error.code,
        exitCode: error.exitCode,
        causeCode: error.cause && error.cause.code
      }));
    }
  `;
  const child = spawnSync(process.execPath, ["-e", script], {
    cwd: work,
    encoding: "utf8",
    env: process.env,
  });
  rmSync(work, { recursive: true, force: true });
  return child;
}

const CLONE_BODY = `  let total = 0;
  for (const item of items) {
    if (item > 10) {
      total += item * 2;
    } else if (item > 5) {
      total += item + 3;
    } else {
      total += item - 1;
    }
  }
  return total;
}
`;

// Two npm workspaces. Workspace `@parity/a` has an unused export, an unused
// file and an unused dependency. Workspace `@parity/b` has an unused export and
// one complex function. One clone group has a copy in each workspace.
function makeParityFixture() {
  const root = mkdtempSync(join(tmpdir(), "fallow-node-parity-"));
  const files = {
    "package.json": { name: "parity-root", private: true, workspaces: ["packages/*"] },
    "packages/a/package.json": {
      name: "@parity/a",
      version: "1.0.0",
      main: "src/index.ts",
      dependencies: { "left-pad": "1.3.0" },
    },
    "packages/b/package.json": { name: "@parity/b", version: "1.0.0", main: "src/index.ts" },
    "packages/a/src/index.ts": `import { sumA } from "./sum";
import { helper } from "./util";
export const usedA = sumA([1, 2, 3]) + helper;
`,
    "packages/a/src/util.ts": "export const helper = 1;\nexport const unusedHelper = 2;\n",
    "packages/a/src/orphan.ts": "export const orphan = 1;\n",
    "packages/a/src/sum.ts": `export function sumA(items: number[]): number {\n${CLONE_BODY}`,
    "packages/b/src/sum.ts": `export function sumB(items: number[]): number {\n${CLONE_BODY}`,
    "packages/b/src/index.ts": `import { sumB } from "./sum";
import { classify } from "./complex";
export const usedB = sumB([4]) + classify(3, 4, 5).length;
`,
    "packages/b/src/complex.ts": `export function classify(a: number, b: number, c: number): string {
  if (a > 0 && b > 0) {
    if (c > a || c > b) {
      return a > b ? "ab" : "ba";
    }
    for (let i = 0; i < a; i++) {
      if (i % 2 === 0 && i % 3 === 0) {
        return "six";
      } else if (i % 5 === 0 || i % 7 === 0) {
        return "odd";
      }
    }
  } else if (a < 0 || b < 0) {
    while (c > 0) {
      c--;
      if (c === 3 && a < -1) {
        return "three";
      }
    }
  }
  switch (a) {
    case 1:
      return "one";
    case 2:
      return "two";
    default:
      return b > 2 ? "many" : "few";
  }
}

export const unusedInB = 3;
`,
  };
  for (const [path, content] of Object.entries(files)) {
    const target = join(root, path);
    mkdirSync(dirname(target), { recursive: true });
    const text = typeof content === "string" ? content : JSON.stringify(content, null, 2) + "\n";
    writeFileSync(target, text);
  }
  return root;
}

// Builds the CLI binary with cargo and returns its path. Cargo reports the
// executable path, so a custom CARGO_TARGET_DIR works too. The parity test
// must never skip, so a failed build or a missing binary throws.
function buildCliBinary() {
  const repoRoot = join(napiRoot, "..", "..");
  const build = spawnSync(
    "cargo",
    ["build", "-p", "fallow-cli", "--bin", "fallow", "--message-format=json"],
    { cwd: repoRoot, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
  );
  if (build.error || build.status !== 0) {
    throw new Error(
      `cargo build -p fallow-cli failed (status ${build.status}): ${build.error ?? build.stderr}`,
    );
  }
  const executable = build.stdout
    .split("\n")
    .filter((line) => line.startsWith("{"))
    .map((line) => JSON.parse(line))
    .find(
      (message) =>
        message.reason === "compiler-artifact" &&
        message.target?.name === "fallow" &&
        message.executable,
    )?.executable;
  if (!executable || !existsSync(executable)) {
    throw new Error(`cargo build did not produce the fallow CLI binary (got ${executable})`);
  }
  return executable;
}

function runCli(binary, root, args) {
  const child = spawnSync(binary, [...args, "--format", "json", "--quiet", "--no-cache"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (child.error || (child.status !== 0 && child.status !== 1)) {
    throw new Error(
      `fallow ${args.join(" ")} failed (status ${child.status}): ${child.error ?? child.stderr}`,
    );
  }
  return JSON.parse(child.stdout);
}

// Top-level arrays in the dead-code report that are not findings.
const DEAD_CODE_NON_FINDING_KEYS = new Set(["workspace_diagnostics", "next_steps"]);

// Identity key per finding: (issue kind, root-relative path, symbol or package
// name, line). Presentation fields such as `actions` stay out of the key.
function deadCodeKeys(report) {
  const keys = [];
  for (const [kind, value] of Object.entries(report)) {
    if (!Array.isArray(value) || DEAD_CODE_NON_FINDING_KEYS.has(kind)) continue;
    for (const item of value) {
      const name = item.export_name ?? item.package_name ?? item.member_name ?? item.name ?? "";
      keys.push(`${kind}|${item.path ?? ""}|${name}|${item.line ?? ""}`);
    }
  }
  return keys.toSorted();
}

function cloneGroupKeys(report) {
  return report.clone_groups
    .map((group) =>
      group.instances
        .map((instance) => `${instance.file}:${instance.start_line}-${instance.end_line}`)
        .toSorted()
        .join(" + "),
    )
    .toSorted();
}

function healthKeys(report) {
  return report.findings.map((item) => `${item.path}|${item.name}|${item.line}`).toSorted();
}

console.log("Testing @fallow-cli/fallow-node...\n");

const root = makeFixture();
const serviceDiff = join(root, "service.diff");
writeFileSync(
  serviceDiff,
  [
    "diff --git a/src/application/service.ts b/src/application/service.ts",
    "--- a/src/application/service.ts",
    "+++ b/src/application/service.ts",
    "@@ -1,5 +1,5 @@",
    " export function usedThing() {",
    "   return 'ok';",
    " }",
    " ",
    "+export const unusedThing = 42;",
    "",
  ].join("\n"),
);

{
  const report = await detectDeadCode({ root, explain: true });
  assert.equal(report.kind, "dead-code");
  assert.equal(report.schema_version, 9);
  assert.ok(report._meta);
  assert.ok(report.unused_exports.some((item) => item.export_name === "unusedThing"));
  console.log("  [PASS] detectDeadCode");
}

{
  const report = await detectDeadCode({
    root,
    diffFile: serviceDiff,
    unusedExports: true,
    threads: 2,
  });
  assert.deepEqual(
    report.unused_exports.map((item) => item.export_name),
    ["unusedThing"],
  );
  console.log("  [PASS] detectDeadCode diffFile");
}

{
  const report = await detectCircularDependencies({ root });
  assert.equal(report.summary.circular_dependencies, 1);
  assert.equal(report.summary.total_issues, 1);
  assert.equal(report.boundary_violations.length, 0);
  console.log("  [PASS] detectCircularDependencies");
}

{
  const report = await detectBoundaryViolations({ root });
  assert.equal(report.summary.boundary_violations, 1);
  assert.equal(report.summary.total_issues, 1);
  assert.equal(report.circular_dependencies.length, 0);
  console.log("  [PASS] detectBoundaryViolations");
}

{
  const report = await detectDuplication({
    root,
    mode: "mild",
    minTokens: 10,
    minLines: 3,
  });
  assert.ok(report.clone_groups.length >= 1);
  console.log("  [PASS] detectDuplication");
}

{
  const report = await detectFeatureFlags({ root, top: 1 });
  assert.equal(report.kind, "feature-flags");
  assert.equal(report.total_flags, 1);
  assert.equal(report.feature_flags.length, 1);
  assert.equal(report.feature_flags[0].flag_name, "FEATURE_ALPHA");
  console.log("  [PASS] detectFeatureFlags");
}

{
  const report = await computeComplexity({
    root,
    complexity: true,
    score: true,
    maxCyclomatic: 1,
    sort: "cyclomatic",
  });
  assert.ok(report.findings.length >= 1);
  assert.ok(report.health_score);
  console.log("  [PASS] computeComplexity");
}

{
  const report = await computeHealth({
    root,
    score: true,
    targets: true,
    effort: "low",
    ownership: true,
    ownershipEmails: "handle",
  });
  assert.ok(report.health_score);
  console.log("  [PASS] computeHealth");
}

{
  let error = null;
  try {
    await detectDeadCode({ root: join(root, "missing-root") });
  } catch (caught) {
    error = caught;
  }
  assert.ok(error);
  assert.equal(error.name, "FallowNodeError");
  assert.equal(error.exitCode, 2);
  assert.equal(error.code, "FALLOW_INVALID_ROOT");
  assert.equal(error.context, "analysis.root");
  assert.match(error.message, /invalid root path/);
  console.log("  [PASS] structured errors");
}

{
  const child = runPanicBoundaryChild();
  assert.equal(child.status, 0, child.stderr);
  assert.match(child.stdout, /CAUGHT:FALLOW_PANIC:FallowNodeError/);
  console.log("  [PASS] panic boundary");
}

{
  const matching = runLoaderCompanionFixture("3.8.0");
  assert.equal(matching.status, 0, matching.stderr);
  const matchingCommand = JSON.parse(matching.stdout);
  if (process.platform === "win32") {
    assert.equal(matchingCommand.binary, process.execPath);
    assert.match(matchingCommand.script, /fallow-type-aware\.mjs$/);
  } else {
    assert.match(matchingCommand.binary, /fallow-type-aware\.mjs$/);
    assert.equal(matchingCommand.script, null);
  }
  assert.equal(matchingCommand.source, "npm-wrapper");

  const mismatched = runLoaderCompanionFixture("3.7.0");
  assert.equal(mismatched.status, 0, mismatched.stderr);
  assert.deepEqual(JSON.parse(mismatched.stdout), { binary: null, script: null, source: null });

  const launchRoot = mkdtempSync(join(tmpdir(), "fallow-node-sidecar-launch-"));
  const launchScript = join(launchRoot, "sidecar.mjs");
  writeFileSync(launchScript, "process.stdout.write('launched');\n");
  const windowsCommand = typeAwareCommand(launchScript, {
    platform: "win32",
    execPath: process.execPath,
  });
  const launched = spawnSync(windowsCommand.binary, [windowsCommand.script], {
    encoding: "utf8",
  });
  rmSync(launchRoot, { recursive: true, force: true });
  assert.equal(launched.status, 0, launched.stderr);
  assert.equal(launched.stdout, "launched");
  console.log("  [PASS] type-aware companion loader");
}

{
  const failedVerification = runSimilarCodeVerificationFailureFixture();
  assert.equal(failedVerification.status, 0, failedVerification.stderr);
  assert.deepEqual(JSON.parse(failedVerification.stdout), {
    name: "FallowNodeError",
    code: "FALLOW_SIMILAR_CODE_PROVIDER_NOT_READY",
    exitCode: 3,
    causeCode: "digest-missing",
  });
  console.log("  [PASS] similar-code companion verification errors");
}

{
  const adversarialRoot = makeAdversarialFixture();
  let error = null;
  try {
    const report = await detectDeadCode({ root: adversarialRoot });
    assert.equal(report.kind, "dead-code");
  } catch (caught) {
    error = caught;
  }
  if (error) {
    assert.equal(error.name, "FallowNodeError");
    assert.equal(typeof error.exitCode, "number");
  }
  console.log("  [PASS] adversarial input stays structured");
}

{
  const parityRoot = makeParityFixture();
  const binary = buildCliBinary();
  const scopes = [
    { label: "no scope", napi: {}, cli: [] },
    {
      label: "workspace @parity/a",
      napi: { workspace: ["@parity/a"] },
      cli: ["--workspace", "@parity/a"],
    },
  ];
  const expected = {
    "no scope": {
      deadCode: [
        "unused_dependencies|packages/a/package.json|left-pad|",
        "unused_exports|packages/a/src/util.ts|unusedHelper|",
        "unused_exports|packages/b/src/complex.ts|unusedInB|",
        "unused_files|packages/a/src/orphan.ts||",
      ],
      health: ["packages/b/src/complex.ts|classify|"],
    },
    "workspace @parity/a": {
      deadCode: [
        "unused_dependencies|packages/a/package.json|left-pad|",
        "unused_exports|packages/a/src/util.ts|unusedHelper|",
        "unused_files|packages/a/src/orphan.ts||",
      ],
      health: [],
    },
  };
  const wholeCloneGroup = ["packages/a/src/sum.ts:1-12 + packages/b/src/sum.ts:1-12"];

  for (const scope of scopes) {
    const options = { root: parityRoot, noCache: true, ...scope.napi };
    const surfaces = {
      deadCode: [
        deadCodeKeys(await detectDeadCode(options)),
        deadCodeKeys(runCli(binary, parityRoot, ["dead-code", ...scope.cli])),
      ],
      clones: [
        cloneGroupKeys(await detectDuplication(options)),
        cloneGroupKeys(runCli(binary, parityRoot, ["dupes", ...scope.cli])),
      ],
      health: [
        healthKeys(await computeHealth(options)),
        healthKeys(runCli(binary, parityRoot, ["health", ...scope.cli])),
      ],
    };
    for (const [analysis, [napiKeys, cliKeys]] of Object.entries(surfaces)) {
      assert.deepEqual(napiKeys, cliKeys, `NAPI and CLI ${analysis} differ (${scope.label})`);
    }
    // The fixture findings must be present, so two empty reports cannot pass.
    const want = expected[scope.label];
    for (const key of want.deadCode) {
      assert.ok(
        surfaces.deadCode[0].some((actual) => actual.startsWith(key)),
        `missing dead-code finding ${key} (${scope.label})`,
      );
    }
    assert.equal(surfaces.deadCode[0].length, want.deadCode.length, `dead-code (${scope.label})`);
    // A workspace scope keeps a clone group whole when one copy is inside it.
    assert.deepEqual(surfaces.clones[0], wholeCloneGroup, `clone groups (${scope.label})`);
    assert.deepEqual(
      surfaces.health[0].map((key) => key.slice(0, key.lastIndexOf("|") + 1)),
      want.health,
      `health findings (${scope.label})`,
    );
  }
  rmSync(parityRoot, { recursive: true, force: true });
  console.log("  [PASS] NAPI findings match the CLI");
}

console.log("\nAll tests passed.");
