import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { API } from "typescript/unstable/sync";

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
  const api = new API();
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
    snapshot = api.updateSnapshot({ openProject: configFileName });
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
