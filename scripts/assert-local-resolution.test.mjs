import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { assertLocalResolution, resolveDependencyDirectory } from "./assert-local-resolution.mjs";

const DEPENDENCY = "fixture-dependency";

const installPackage = (root, manifest = {}) => {
  const directory = join(root, "node_modules", DEPENDENCY);
  mkdirSync(directory, { recursive: true });
  writeFileSync(
    join(directory, "package.json"),
    `${JSON.stringify({ name: DEPENDENCY, version: "1.0.0", ...manifest })}\n`,
  );
  writeFileSync(join(directory, "index.js"), "module.exports = {};\n");
  return directory;
};

/**
 * Build the escape layout: an outer checkout that owns the install and a
 * nested checkout that does not, mirroring a worktree inside a clone.
 */
const nestedCheckout = () => {
  const outer = mkdtempSync(join(tmpdir(), "fallow-resolution-"));
  const checkout = join(outer, ".git-worktrees", "nested");
  mkdirSync(join(checkout, "scripts"), { recursive: true });
  return { outer, checkout, entrypoint: join(checkout, "scripts", "entrypoint.mjs") };
};

test("resolution escaping the checkout names the foreign path and the install command", () => {
  const { outer, checkout, entrypoint } = nestedCheckout();
  const foreign = installPackage(outer);
  try {
    assert.throws(
      () =>
        assertLocalResolution({
          dependency: DEPENDENCY,
          resolveFrom: entrypoint,
          repoRoot: checkout,
          installCommand: "npm ci",
        }),
      (error) =>
        error.message.includes(foreign) &&
        error.message.includes(checkout) &&
        error.message.includes("npm ci"),
    );
  } finally {
    rmSync(outer, { recursive: true, force: true });
  }
});

test("a local install alongside an ancestor install passes silently", () => {
  const { outer, checkout, entrypoint } = nestedCheckout();
  installPackage(outer);
  installPackage(checkout);
  try {
    assert.doesNotThrow(() =>
      assertLocalResolution({
        dependency: DEPENDENCY,
        resolveFrom: entrypoint,
        repoRoot: checkout,
        installCommand: "npm ci",
      }),
    );
  } finally {
    rmSync(outer, { recursive: true, force: true });
  }
});

test("a dependency hiding package.json behind exports still resolves", () => {
  const { outer, checkout, entrypoint } = nestedCheckout();
  const local = installPackage(checkout, { exports: { ".": "./index.js" } });
  try {
    assert.equal(resolveDependencyDirectory(DEPENDENCY, entrypoint), local);
    assert.doesNotThrow(() =>
      assertLocalResolution({
        dependency: DEPENDENCY,
        resolveFrom: entrypoint,
        repoRoot: checkout,
        installCommand: "npm ci",
      }),
    );
  } finally {
    rmSync(outer, { recursive: true, force: true });
  }
});

test("a dependency missing everywhere reports the install command", () => {
  const { outer, checkout, entrypoint } = nestedCheckout();
  try {
    assert.equal(resolveDependencyDirectory(DEPENDENCY, entrypoint), null);
    assert.throws(
      () =>
        assertLocalResolution({
          dependency: DEPENDENCY,
          resolveFrom: entrypoint,
          repoRoot: checkout,
          installCommand: "pnpm --dir editors/vscode install",
        }),
      /is not installed anywhere Node can reach.*pnpm --dir editors\/vscode install/su,
    );
  } finally {
    rmSync(outer, { recursive: true, force: true });
  }
});

test("the repository pins resolve inside this checkout", () => {
  assert.doesNotThrow(() => assertLocalResolution({ dependency: "oxlint" }));
  assert.doesNotThrow(() => assertLocalResolution({ dependency: "oxfmt" }));
});
