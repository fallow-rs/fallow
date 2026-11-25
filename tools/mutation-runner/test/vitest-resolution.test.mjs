import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

import { startCompanion } from "./support/companion-process.mjs";

// Vitest 4.0 is outside the supported range. The companion refuses it with a
// message that names the version before it loads Vitest, so a `package.json`
// alone shows which Vitest install the companion found.
const FOUND = "4.0.0";
const OTHER = "4.0.18";

const writeJson = (path, value) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(value));
};

// A toolchain such as Vite+ lists Vitest as its own dependency.
const TOOLCHAIN = { name: "toolchain", dependencies: { vitest: "4.0.0" } };

const writeVitest = (dir, version) =>
  writeJson(join(dir, "vitest/package.json"), { name: "vitest", version });

const initError = async (root) => {
  const companion = startCompanion();
  const runDir = mkdtempSync(join(tmpdir(), "fallow-mutation-run-"));
  try {
    const response = await companion.request({
      v: 1,
      id: 1,
      type: "init",
      root,
      runner: "vitest",
      config_path: null,
      run_dir: runDir,
      path_map: {},
      options: { test_files: [], env: {} },
    });
    assert.equal(response.type, "error", JSON.stringify(response));
    return response.error;
  } finally {
    companion.stop();
    rmSync(runDir, { recursive: true, force: true });
  }
};

const withRoot = (body) => async () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-mutation-resolve-"));
  try {
    await body(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

const assertFound = (error, version) => {
  assert.equal(error.code, "runner-version", error.message);
  assert.match(error.message, new RegExp(`^vitest ${version.replaceAll(".", "\\.")} `, "u"));
};

test(
  "init finds the Vitest of a listed dependency when the root has no Vitest",
  withRoot(async (root) => {
    writeJson(join(root, "package.json"), { devDependencies: { toolchain: "1.0.0" } });
    writeJson(join(root, "node_modules/toolchain/package.json"), TOOLCHAIN);
    writeVitest(join(root, "node_modules/toolchain/node_modules"), FOUND);
    assertFound(await initError(root), FOUND);
  }),
);

test(
  "init follows the symlink of a pnpm dependency to its sibling Vitest",
  withRoot(async (root) => {
    const store = join(root, "node_modules/.pnpm/toolchain@1.0.0/node_modules");
    writeJson(join(root, "package.json"), { devDependencies: { toolchain: "1.0.0" } });
    writeJson(join(store, "toolchain/package.json"), TOOLCHAIN);
    writeVitest(store, FOUND);
    symlinkSync(join(store, "toolchain"), join(root, "node_modules/toolchain"), "dir");
    assertFound(await initError(root), FOUND);
  }),
);

test(
  "init prefers the Vitest of the project root over the Vitest of a dependency",
  withRoot(async (root) => {
    writeJson(join(root, "package.json"), { devDependencies: { toolchain: "1.0.0" } });
    writeVitest(join(root, "node_modules"), FOUND);
    writeJson(join(root, "node_modules/toolchain/package.json"), TOOLCHAIN);
    writeVitest(join(root, "node_modules/toolchain/node_modules"), OTHER);
    assertFound(await initError(root), FOUND);
  }),
);

test(
  "init ignores the Vitest of a package that the project does not list",
  withRoot(async (root) => {
    writeJson(join(root, "package.json"), { devDependencies: {} });
    writeJson(join(root, "node_modules/toolchain/package.json"), TOOLCHAIN);
    writeVitest(join(root, "node_modules/toolchain/node_modules"), FOUND);
    assert.equal((await initError(root)).code, "runner-not-found");
  }),
);

test(
  "init skips a listed package that reaches a hoisted Vitest but does not declare it",
  withRoot(async (root) => {
    const pnpm = join(root, "node_modules/.pnpm");
    const toolchain = join(pnpm, "toolchain@1.0.0/node_modules");
    const library = join(pnpm, "library@1.0.0/node_modules");
    writeJson(join(root, "package.json"), {
      dependencies: { library: "1.0.0" },
      devDependencies: { toolchain: "1.0.0" },
    });
    writeJson(join(library, "library/package.json"), { name: "library" });
    writeJson(join(toolchain, "toolchain/package.json"), TOOLCHAIN);
    writeVitest(toolchain, FOUND);
    writeVitest(join(pnpm, "node_modules"), OTHER);
    symlinkSync(join(library, "library"), join(root, "node_modules/library"), "dir");
    symlinkSync(join(toolchain, "toolchain"), join(root, "node_modules/toolchain"), "dir");
    assertFound(await initError(root), FOUND);
  }),
);

test(
  "init ignores a hoisted Vitest that no listed package declares",
  withRoot(async (root) => {
    const library = join(root, "node_modules/.pnpm/library@1.0.0/node_modules");
    writeJson(join(root, "package.json"), { dependencies: { library: "1.0.0" } });
    writeJson(join(library, "library/package.json"), { name: "library" });
    writeVitest(join(root, "node_modules/.pnpm/node_modules"), OTHER);
    symlinkSync(join(library, "library"), join(root, "node_modules/library"), "dir");
    assert.equal((await initError(root)).code, "runner-not-found");
  }),
);

test(
  "init ignores the Vitest that a listed package has only as a dev dependency",
  withRoot(async (root) => {
    writeJson(join(root, "package.json"), { dependencies: { ui: "workspace:*" } });
    writeJson(join(root, "node_modules/ui/package.json"), {
      name: "ui",
      devDependencies: { vitest: "4.0.18" },
    });
    writeVitest(join(root, "node_modules/ui/node_modules"), OTHER);
    assert.equal((await initError(root)).code, "runner-not-found");
  }),
);

for (const [field, code] of [
  ["optionalDependencies", "runner-version"],
  ["peerDependencies", "runner-not-found"],
]) {
  test(
    `init ${code === "runner-version" ? "uses" : "ignores"} a listed package with vitest in ${field}`,
    withRoot(async (root) => {
      writeJson(join(root, "package.json"), { devDependencies: { plugin: "1.0.0" } });
      writeJson(join(root, "node_modules/plugin/package.json"), {
        name: "plugin",
        [field]: { vitest: "4.0.0" },
      });
      writeVitest(join(root, "node_modules/plugin/node_modules"), FOUND);
      assert.equal((await initError(root)).code, code);
    }),
  );
}
