import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { strict as assert } from "node:assert";
import { execFileSync } from "node:child_process";

import {
  computeComplexity,
  computeHealth,
  detectBoundaryViolations,
  detectCircularDependencies,
  detectDeadCode,
  detectDuplication,
  detectFeatureFlags,
} from "./index.js";

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
  assert.equal(report.schema_version, 7);
  assert.ok(report._meta);
  assert.ok(report.unused_exports.some((item) => item.export_name === "unusedThing"));
  console.log("  [PASS] detectDeadCode");
}

{
  const report = await detectDeadCode({ root, legacyEnvelope: true });
  assert.equal(report.kind, undefined);
  assert.equal(report.schema_version, 7);
  console.log("  [PASS] detectDeadCode legacyEnvelope");
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
  assert.match(error.message, /analysis root does not exist/);
  console.log("  [PASS] structured errors");
}

console.log("\nAll tests passed.");
