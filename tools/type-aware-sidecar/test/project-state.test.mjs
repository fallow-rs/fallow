import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs, { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { syncBuiltinESMExports } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { createApi } from "../src/backend.mjs";

import { projectState } from "../src/project-state.mjs";

const createRoot = () => mkdtempSync(path.join(tmpdir(), "fallow-project-state-"));

const write = (root, relativePath, contents) => {
  const fileName = path.join(root, relativePath);
  mkdirSync(path.dirname(fileName), { recursive: true });
  writeFileSync(fileName, contents);
};

const BASE_CONFIG = JSON.stringify({
  compilerOptions: {
    allowArbitraryExtensions: true,
    module: "ESNext",
    moduleResolution: "Bundler",
    skipLibCheck: true,
    strict: true,
  },
  include: ["src/**/*.ts"],
});

const AMBIENT_SVELTE_MODULE = `declare module "*.svelte" {
  const component: unknown;
  export default component;
}
`;

const inspectProject = (files) => {
  const root = createRoot();
  const api = createApi();
  let snapshot;
  try {
    write(root, "tsconfig.json", BASE_CONFIG);
    write(root, "src/svelte.d.ts", AMBIENT_SVELTE_MODULE);
    write(
      root,
      "src/button.svelte",
      '<script lang="ts" module>export const buttonVariants = "";</script>\n',
    );
    Object.entries(files).forEach(([fileName, contents]) => write(root, fileName, contents));
    const configFileName = path.join(root, "tsconfig.json");
    snapshot = api.updateSnapshot({ openProjects: [configFileName] });
    const project = snapshot.getProject(configFileName);
    assert.ok(project, "expected TypeScript-Go to open the fixture project");
    const state = projectState(root, project, "explicit");
    return {
      status: state.status,
      reasonCode: state.reason_code,
      blockingDiagnosticCount: state.blocking_diagnostic_count,
    };
  } finally {
    snapshot?.dispose();
    api.close();
    rmSync(root, { recursive: true, force: true });
  }
};

const EXTERNAL_BIND_DIAGNOSTIC = {
  "node_modules/external/index.d.ts": "export const arguments: never;\n",
  "node_modules/external/package.json": JSON.stringify({
    name: "external",
    type: "module",
    types: "index.d.ts",
  }),
};

test("classifies unresolved named Svelte imports despite external declaration diagnostics", () => {
  const state = inspectProject({
    ...EXTERNAL_BIND_DIAGNOSTIC,
    "src/index.ts": [
      'import "external";',
      'import Button, { buttonVariants, type ButtonProps } from "./button.svelte";',
      "export { Button, buttonVariants, type ButtonProps };",
      "",
    ].join("\n"),
  });

  assert.equal(state.status, "unavailable");
  assert.equal(state.reasonCode, "svelte-virtual-module-exports");
  assert.equal(state.blockingDiagnosticCount, 0);
});

test("classifies unresolved named Svelte re-exports", () => {
  const state = inspectProject({
    "src/index.ts":
      'export { default as Button, buttonVariants, type ButtonProps } from "./button.svelte";\n',
  });

  assert.equal(state.status, "unavailable");
  assert.equal(state.reasonCode, "svelte-virtual-module-exports");
});

test("classifies unproven Svelte export stars", () => {
  const state = inspectProject({
    "src/index.ts": 'export * from "./button.svelte";\n',
  });

  assert.equal(state.status, "unavailable");
  assert.equal(state.reasonCode, "svelte-virtual-module-exports");
});

test("keeps external declaration diagnostics blocking without a Svelte named-export gap", () => {
  const state = inspectProject({
    ...EXTERNAL_BIND_DIAGNOSTIC,
    "src/index.ts": [
      'import "external";',
      'import Button from "./button.svelte";',
      "export { Button };",
      "",
    ].join("\n"),
  });

  assert.equal(state.status, "unavailable");
  assert.equal(state.reasonCode, "blocking-diagnostics");
  assert.ok(state.blockingDiagnosticCount > 0);
});

test("does not treat a partial ambient Svelte export set as proven", () => {
  const state = inspectProject({
    "src/svelte.d.ts": [
      'declare module "*.svelte" {',
      "  export const known: string;",
      "  const component: unknown;",
      "  export default component;",
      "}",
      "",
    ].join("\n"),
    "src/index.ts": 'export * from "./button.svelte";\n',
  });

  assert.equal(state.status, "unavailable");
  assert.equal(state.reasonCode, "svelte-virtual-module-exports");
});

test("classifies unproven Svelte namespace re-exports", () => {
  const state = inspectProject({
    "src/index.ts": 'export * as button from "./button.svelte";\n',
  });

  assert.equal(state.status, "unavailable");
  assert.equal(state.reasonCode, "svelte-virtual-module-exports");
});

test("keeps declaration-backed named Svelte targets complete", () => {
  const state = inspectProject({
    "src/button.d.svelte.ts": [
      "export declare const buttonVariants: string;",
      "export interface ButtonProps { disabled?: boolean }",
      "declare const Button: unknown;",
      "export default Button;",
      "",
    ].join("\n"),
    "src/index.ts":
      'export { default as Button, buttonVariants, type ButtonProps } from "./button.svelte";\n',
  });

  assert.equal(state.status, "complete");
  assert.equal(state.reasonCode, null);
});

test("keeps declaration-backed Svelte export stars complete", () => {
  const state = inspectProject({
    "src/button.d.svelte.ts": [
      "export declare const buttonVariants: string;",
      "declare const Button: unknown;",
      "export default Button;",
      "",
    ].join("\n"),
    "src/index.ts": 'export * from "./button.svelte";\n',
  });

  assert.equal(state.status, "complete");
  assert.equal(state.reasonCode, null);
});

test("keeps declaration-backed default-only Svelte namespace exports complete", () => {
  const state = inspectProject({
    "src/button.d.svelte.ts": ["declare const Button: unknown;", "export default Button;", ""].join(
      "\n",
    ),
    "src/index.ts": 'export * as button from "./button.svelte";\n',
  });

  assert.equal(state.status, "complete");
  assert.equal(state.reasonCode, null);
});

test("prioritizes project-local structural diagnostics over Svelte gaps", () => {
  const state = inspectProject({
    "src/index.ts": [
      "export const arguments = 1;",
      'export { buttonVariants } from "./button.svelte";',
      "",
    ].join("\n"),
  });

  assert.equal(state.status, "unavailable");
  assert.equal(state.reasonCode, "blocking-diagnostics");
  assert.ok(state.blockingDiagnosticCount > 0);
});

test("names a file outside every tsconfig as the inferred project", () => {
  const root = createRoot();
  const api = createApi({ cwd: root });
  let snapshot;
  try {
    write(root, "loose.ts", "export const loose = 1;\n");
    const fileName = path.join(root, "loose.ts");
    snapshot = api.updateSnapshot({ openFiles: [fileName] });
    const state = projectState(root, snapshot.getDefaultProjectForFile(fileName), "auto");
    assert.equal(state.config, "<inferred>");
  } finally {
    snapshot?.dispose();
    api.close();
    rmSync(root, { recursive: true, force: true });
  }
});

const sharedExtendsChain = (root, length, lastConfig) => {
  const name = (index) => `c${String(index).padStart(2, "0")}.json`;
  write(
    root,
    "tsconfig.json",
    JSON.stringify({ extends: [`./${name(1)}`, `./${name(1)}`], include: ["src/**/*.ts"] }),
  );
  for (let index = 1; index < length; index += 1) {
    const next = `./${name(index + 1)}`;
    write(root, name(index), JSON.stringify({ extends: [next, next] }));
  }
  write(root, name(length), JSON.stringify(lastConfig));
  write(root, "src/index.ts", "export const value = 1;\n");
  return path.join(root, name(length));
};

// The config hash of a chain of `length` configs in which every config
// extends the next one twice, so 2^length paths reach the last config. Also
// counts the reads of the last config and the hashed text.
const sharedChainConfigHash = (length, lastConfig) => {
  const root = createRoot();
  const api = createApi({ cwd: root });
  const { readFileSync } = fs;
  const { createHash } = crypto;
  let snapshot;
  try {
    const lastFileName = sharedExtendsChain(root, length, lastConfig);
    const configFileName = path.join(root, "tsconfig.json");
    snapshot = api.updateSnapshot({ openProjects: [configFileName] });
    const project = snapshot.getProject(configFileName);
    assert.ok(project, "expected TypeScript-Go to open the fixture project");
    let lastReads = 0;
    let hashedLength = 0;
    fs.readFileSync = (fileName, ...rest) => {
      if (path.resolve(String(fileName)) === lastFileName) lastReads += 1;
      return readFileSync(fileName, ...rest);
    };
    crypto.createHash = (...options) => {
      const hash = createHash(...options);
      const update = hash.update.bind(hash);
      hash.update = (data, ...rest) => {
        hashedLength += data.length;
        return update(data, ...rest);
      };
      return hash;
    };
    syncBuiltinESMExports();
    const state = projectState(root, project, "explicit");
    return { hash: state.effective_config_hash, lastReads, hashedLength };
  } finally {
    fs.readFileSync = readFileSync;
    crypto.createHash = createHash;
    syncBuiltinESMExports();
    snapshot?.dispose();
    api.close();
    rmSync(root, { recursive: true, force: true });
  }
};

test("reads a config that many extends paths share a bounded number of times", () => {
  const plain = sharedChainConfigHash(16, { compilerOptions: { strict: true } });
  assert.ok(plain.lastReads <= 4, `the last config was read ${plain.lastReads} times`);

  const changed = sharedChainConfigHash(16, { compilerOptions: { strict: false } });
  assert.notEqual(plain.hash, changed.hash, "the hash must cover the shared base config");
});

test("hashes a large config that many extends paths share a bounded amount of text", () => {
  // 1,024 paths reach a 60 KB config, so a copy per path is 60 MB of text.
  const large = {
    fallowFixture: Array.from({ length: 2_000 }, (_, index) => `${index}`.repeat(8)),
  };
  const { hashedLength } = sharedChainConfigHash(10, large);
  assert.ok(hashedLength < 16 * 1024 * 1024, `the config hash hashed ${hashedLength} characters`);
});

test("keeps the config hash of a small config graph", () => {
  // A saved type-aware baseline stores this hash. A change of the hash form
  // makes every saved baseline incompatible, so it must be a deliberate one.
  const root = createRoot();
  const api = createApi({ cwd: root });
  let snapshot;
  try {
    write(root, "base.json", JSON.stringify({ compilerOptions: { strict: true } }));
    write(
      root,
      "tsconfig.json",
      JSON.stringify({
        extends: "./base.json",
        compilerOptions: { module: "ESNext", moduleResolution: "Bundler", skipLibCheck: true },
        include: ["src/**/*.ts"],
        references: [{ path: "./pkg" }],
      }),
    );
    write(
      root,
      "pkg/tsconfig.json",
      JSON.stringify({ extends: "../base.json", compilerOptions: { composite: true } }),
    );
    write(root, "src/index.ts", "export const value = 1;\n");
    const configFileName = path.join(root, "tsconfig.json");
    snapshot = api.updateSnapshot({ openProjects: [configFileName] });
    const project = snapshot.getProject(configFileName);
    assert.ok(project, "expected TypeScript-Go to open the fixture project");
    assert.equal(
      projectState(root, project, "explicit").effective_config_hash,
      "sha256:5a0955b89850beedf81ed8df2d9aad4e24f9d6d65fbaa41cca6d28af24ed272a",
    );
  } finally {
    snapshot?.dispose();
    api.close();
    rmSync(root, { recursive: true, force: true });
  }
});
