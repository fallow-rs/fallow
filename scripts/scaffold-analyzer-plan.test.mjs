import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import {
  assertPlanPath,
  assertWritablePlanPath,
  errorMessage,
  parseArgs,
  planTemplate,
  writePlan,
} from "./scaffold-analyzer-plan.mjs";

const SCRIPT_PATH = fileURLToPath(new URL("./scaffold-analyzer-plan.mjs", import.meta.url));
const OUTSIDE_ERROR = /refusing to write outside \.plans\/analyzers/u;

const withTempDir = (run) => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "scaffold-analyzer-plan-")));
  try {
    return run(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

test("parseArgs accepts one kebab-case code", () => {
  assert.deepEqual(parseArgs(["unused-store-member"]), {
    ok: true,
    data: { code: "unused-store-member", force: false },
  });
});

test("parseArgs reads --force in any position", () => {
  assert.deepEqual(parseArgs(["--force", "dead-code"]), {
    ok: true,
    data: { code: "dead-code", force: true },
  });
  assert.deepEqual(parseArgs(["a1", "--force"]), {
    ok: true,
    data: { code: "a1", force: true },
  });
});

test("parseArgs rejects zero or many codes", () => {
  const expected = { ok: false, error: "expected exactly one analyzer code" };
  assert.deepEqual(parseArgs([]), expected);
  assert.deepEqual(parseArgs(["--force"]), expected);
  assert.deepEqual(parseArgs(["one", "two"]), expected);
});

test("parseArgs rejects codes that are not lowercase kebab case", () => {
  for (const code of ["Unused", "1st-rule", "trailing-", "double--dash", "snake_case", "-x", ""]) {
    const parsed = parseArgs([code]);
    assert.equal(parsed.ok, false, code);
    assert.match(parsed.error, /lowercase kebab case/u);
  }
});

test("planTemplate names the code and the full validation commands", () => {
  const plan = planTemplate("unused-store-member");
  assert.ok(plan.startsWith("# unused-store-member\n\nStatus: draft\n"));
  assert.ok(plan.includes("`cargo test --workspace --lib --bins --tests --examples`"));
  assert.ok(plan.includes("`cargo check --workspace --benches`"));
  assert.ok(plan.endsWith("--format json --quiet`\n"));
});

test("assertPlanPath rejects paths outside the plan directory", () => {
  const planDir = join(tmpdir(), "plans", "analyzers");
  assert.doesNotThrow(() => assertPlanPath(planDir, join(planDir, "x.md")));
  assert.throws(() => assertPlanPath(planDir, join(planDir, "..", "x.md")), OUTSIDE_ERROR);
  assert.throws(() => assertPlanPath(planDir, join(tmpdir(), "x.md")), OUTSIDE_ERROR);
});

test("writePlan creates the plan directory and the plan file", () => {
  withTempDir((root) => {
    const planDir = join(root, "nested", "analyzers");
    const targetPath = writePlan({ code: "dead-code", force: false, planDir });
    assert.equal(targetPath, join(planDir, "dead-code.md"));
    assert.equal(readFileSync(targetPath, "utf8"), planTemplate("dead-code"));
  });
});

test("writePlan refuses to overwrite without force and overwrites with force", () => {
  withTempDir((root) => {
    const targetPath = join(root, "dead-code.md");
    writeFileSync(targetPath, "old");
    assert.throws(
      () => writePlan({ code: "dead-code", force: false, planDir: root }),
      (error) => errorMessage(error) === "plan already exists, pass --force to overwrite it",
    );
    assert.equal(readFileSync(targetPath, "utf8"), "old");

    writePlan({ code: "dead-code", force: true, planDir: root });
    assert.equal(readFileSync(targetPath, "utf8"), planTemplate("dead-code"));
  });
});

test("assertWritablePlanPath refuses a symlink plan file", () => {
  withTempDir((root) => {
    writeFileSync(join(root, "real.md"), "real");
    symlinkSync(join(root, "real.md"), join(root, "link.md"));
    assert.throws(
      () => writePlan({ code: "link", force: true, planDir: root }),
      /refusing to overwrite a symlink plan file/u,
    );
    assert.equal(readFileSync(join(root, "real.md"), "utf8"), "real");
  });
});

test("assertWritablePlanPath refuses a directory at the plan path", () => {
  withTempDir((root) => {
    mkdirSync(join(root, "dir.md"));
    assert.throws(
      () => assertWritablePlanPath(root, join(root, "dir.md")),
      /refusing to overwrite a non-file plan path/u,
    );
  });
});

test("assertWritablePlanPath refuses a target in another directory", () => {
  withTempDir((root) => {
    mkdirSync(join(root, "plans"));
    mkdirSync(join(root, "other"));
    assert.throws(
      () => assertWritablePlanPath(join(root, "plans"), join(root, "other", "x.md")),
      OUTSIDE_ERROR,
    );
  });
});

test("assertWritablePlanPath accepts a new file in the plan directory", () => {
  withTempDir((root) => {
    assert.doesNotThrow(() => assertWritablePlanPath(root, join(root, "new.md")));
  });
});

test("errorMessage formats errors and other thrown values", () => {
  assert.equal(errorMessage(new Error("boom")), "boom");
  assert.equal(errorMessage("text"), "text");
  assert.equal(errorMessage(null), "null");
});

test("the CLI prints usage and exits 1 for invalid arguments", () => {
  const result = spawnSync(process.execPath, [SCRIPT_PATH, "Bad_Code"], { encoding: "utf8" });
  assert.equal(result.status, 1);
  assert.equal(result.stdout, "");
  assert.equal(
    result.stderr,
    "analyzer code must be lowercase kebab case, for example unused-store-member\n" +
      "Usage: npm run scaffold:analyzer -- <kebab-code> [--force]\n",
  );
});
