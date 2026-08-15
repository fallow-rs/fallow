import assert from "node:assert/strict";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { assertLocalResolution, resolveDependencyDirectory } from "./assert-local-resolution.mjs";

const DEPENDENCY = "fixture-dependency";
const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

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

const installFakeOxlint = (root) => {
  const packageDirectory = join(root, "node_modules", "oxlint");
  const binDirectory = join(root, "node_modules", ".bin");
  mkdirSync(packageDirectory, { recursive: true });
  mkdirSync(binDirectory, { recursive: true });
  writeFileSync(
    join(packageDirectory, "package.json"),
    `${JSON.stringify({ name: "oxlint", version: "0.0.0" })}\n`,
  );
  if (process.platform === "win32") {
    writeFileSync(join(binDirectory, "oxlint.cmd"), "@exit /b 0\r\n");
  } else {
    const binary = join(binDirectory, "oxlint");
    writeFileSync(binary, "#!/bin/sh\nexit 0\n");
    chmodSync(binary, 0o755);
  }
};

const runNpmIgnoringScripts = (cwd) => {
  if (process.platform === "win32") {
    return spawnSync(
      process.env.ComSpec ?? "cmd.exe",
      ["/d", "/s", "/c", "npm run --ignore-scripts lint:js"],
      { cwd, encoding: "utf8" },
    );
  }
  return spawnSync("npm", ["run", "--ignore-scripts", "lint:js"], {
    cwd,
    encoding: "utf8",
  });
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
  assert.doesNotThrow(() => assertLocalResolution({ dependency: "@commitlint/cli" }));
});

test("ignore-scripts cannot bypass the lint script guard", () => {
  const { outer, checkout } = nestedCheckout();
  const packageJson = JSON.parse(readFileSync(join(REPO_ROOT, "package.json"), "utf8"));
  installFakeOxlint(outer);
  writeFileSync(
    join(checkout, "package.json"),
    `${JSON.stringify({
      private: true,
      type: "module",
      scripts: { "lint:js": packageJson.scripts["lint:js"] },
    })}\n`,
  );
  copyFileSync(
    join(REPO_ROOT, "scripts", "assert-local-resolution.mjs"),
    join(checkout, "scripts", "assert-local-resolution.mjs"),
  );
  copyFileSync(
    join(REPO_ROOT, "scripts", "cli-main.mjs"),
    join(checkout, "scripts", "cli-main.mjs"),
  );

  try {
    const result = runNpmIgnoringScripts(checkout);
    assert.notEqual(result.status, 0, `${result.stdout}\n${result.stderr}`);
    assert.match(`${result.stdout}\n${result.stderr}`, /outside this checkout/u);
  } finally {
    rmSync(outer, { recursive: true, force: true });
  }
});
