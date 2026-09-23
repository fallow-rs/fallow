import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { assertScopesAgree, lintScopes, stagedLintTargets } from "./js-lint-scopes.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const packageJson = JSON.parse(readFileSync(join(repoRoot, "package.json"), "utf8"));
const helper = join(repoRoot, "scripts/js-lint-scopes.mjs");

const withScript = (name, replace) => ({
  ...packageJson,
  scripts: { ...packageJson.scripts, [name]: replace(packageJson.scripts[name]) },
});

test("lint:js holds the scope list", () => {
  const scopes = lintScopes(packageJson);
  assert.ok(scopes.includes("scripts"));
  assert.ok(scopes.includes("commitlint.config.mjs"));
  assert.equal(
    scopes.some((scope) => scope.startsWith("-")),
    false,
  );
});

test("the lint and format scripts use the same scope list", () => {
  assertScopesAgree(packageJson);
});

test("a scope that only one script holds fails the check", () => {
  const drifted = withScript("fmt:js:check", (script) => `${script} new-tree`);
  assert.throws(() => assertScopesAgree(drifted), /fmt:js:check.*new-tree/su);

  const missing = withScript("fmt:js", (script) => script.replace(" scripts ", " "));
  assert.throws(() => assertScopesAgree(missing), /fmt:js\b.*scripts/su);
});

test("staged paths are filtered to JavaScript files inside a scope", () => {
  const scopes = ["npm", "editors/vscode/src", "commitlint.config.mjs"];
  const staged = [
    "npm/fallow/index.js",
    "npm/fallow/README.md",
    "npmextra/index.js",
    "editors/vscode/src/extension.ts",
    "editors/vscode/test/extension.ts",
    "commitlint.config.mjs",
    "crates/cli/src/main.rs",
  ];
  assert.deepEqual(stagedLintTargets(staged, scopes), [
    "npm/fallow/index.js",
    "editors/vscode/src/extension.ts",
    "commitlint.config.mjs",
  ]);
});

test("the command line filters standard input with the package.json list", () => {
  const result = spawnSync(process.execPath, [helper, "--filter-staged"], {
    cwd: repoRoot,
    input: "scripts/vendor-skills.mjs\ncrates/cli/src/main.rs\nnot-a-scope/a.js\n",
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, "scripts/vendor-skills.mjs\n");
});

test("the pre-commit hook reads the scope list from the helper", () => {
  const hook = readFileSync(join(repoRoot, ".githooks/pre-commit"), "utf8");
  assert.match(hook, /node scripts\/js-lint-scopes\.mjs --filter-staged/u);
  assert.doesNotMatch(hook, /grep -E '\^\(npm\//u, "the hook must not hold its own copy");
});
