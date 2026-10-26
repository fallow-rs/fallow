import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { ANALYSIS_OPERATION, WIRE_PROTOCOL_VERSION } from "../src/generated-protocol.mjs";

const executable = fileURLToPath(new URL("../fallow-type-aware.mjs", import.meta.url));
const symbolQuery = (id, file, name) => ({
  id,
  operation: "symbol-use",
  symbol: {
    path: file,
    namespace: "value",
    declaration_kind: "variable",
    exported_name: name,
    local_name: name,
    line: 1,
    col: 13,
  },
});

const run = (root, queries, projects = ["tsconfig.json"]) => {
  const child = spawnSync(process.execPath, [executable], {
    input: JSON.stringify({
      protocol_version: WIRE_PROTOCOL_VERSION,
      operation: ANALYSIS_OPERATION,
      root,
      projects,
      evidence_limit: 40,
      queries,
    }),
    encoding: "utf8",
    timeout: 30_000,
  });
  assert.equal(child.status, 0, `semantic batch must survive component queries: ${child.stderr}`);
  return JSON.parse(child.stdout);
};

for (const [extension, source] of [
  ["vue", '<script lang="ts">export const widget = 1;</script>'],
  ["astro", "---\nexport const widget = 1;\n---\n<h1>Widget</h1>"],
  ["svelte", '<script lang="ts" context="module">export const widget = 1;</script>'],
  ["mdx", "export const widget = 1;\n\n# Widget"],
]) {
  test(`${extension} queries abstain while TypeScript queries still resolve`, () => {
    const root = mkdtempSync(path.join(tmpdir(), "fallow-component-queries-"));
    try {
      writeFileSync(
        path.join(root, "tsconfig.json"),
        JSON.stringify({ compilerOptions: { strict: true }, include: ["*.ts"] }),
      );
      writeFileSync(
        path.join(root, "api.ts"),
        "export const used = 1;\nexport const unused = 2;\n",
      );
      writeFileSync(path.join(root, "main.ts"), 'import { used } from "./api";\nvoid used;\n');
      writeFileSync(path.join(root, `Widget.${extension}`), source);
      const unused = symbolQuery(2, "api.ts", "unused");
      unused.symbol.line = 2;
      const response = run(root, [
        symbolQuery(0, `Widget.${extension}`, "widget"),
        symbolQuery(1, "api.ts", "used"),
        unused,
      ]);
      assert.deepEqual(
        response.results.map(({ query_id, assertion, status, reason_code }) => [
          query_id,
          assertion,
          status,
          reason_code,
        ]),
        [
          [0, "no-confirmed-use", "unavailable", "unsupported-syntax"],
          [1, "confirmed-used", "complete", null],
          [2, "confirmed-no-static-references", "complete", null],
        ],
      );
      assert.equal(response.results[1].evidence[0].path, "main.ts");
      assert.deepEqual(response.results[0].omissions, [
        { reason_code: "unsupported-syntax", count: 1 },
      ]);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

for (const operation of ["symbol-use", "symbol-trace", "symbol-impact"]) {
  test(`${operation} abstains for component-only input without a TypeScript project`, () => {
    const root = mkdtempSync(path.join(tmpdir(), "fallow-component-only-"));
    try {
      writeFileSync(path.join(root, "Widget.astro"), "---\nexport const widget = 1;\n---\n");
      const component = { ...symbolQuery(0, "Widget.astro", "widget"), operation };
      const missingProject = { ...symbolQuery(1, "missing.ts", "missing"), operation };
      const response = run(root, [component, missingProject], []);
      assert.deepEqual(
        response.results.map(({ query_id, status, reason_code }) => [
          query_id,
          status,
          reason_code,
        ]),
        [
          [0, "unavailable", "unsupported-syntax"],
          [1, "unavailable", "no-project"],
        ],
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

for (const extension of [
  "ts",
  "tsx",
  "mts",
  "cts",
  "js",
  "jsx",
  "mjs",
  "cjs",
  "d.ts",
  "d.mts",
  "d.cts",
]) {
  test(`supported ${extension} queries survive a mixed component batch`, () => {
    const root = mkdtempSync(path.join(tmpdir(), "fallow-supported-queries-"));
    try {
      const file = `api.${extension}`;
      writeFileSync(
        path.join(root, "tsconfig.json"),
        JSON.stringify({
          compilerOptions: { strict: true, allowJs: true, noEmit: true, jsx: "preserve" },
          files: [file],
        }),
      );
      const declaration = extension.startsWith("d.");
      writeFileSync(
        path.join(root, file),
        declaration ? "export declare const unused: number;\n" : "export const unused = 1;\n",
      );
      writeFileSync(
        path.join(root, "Widget.vue"),
        '<script lang="ts">export const widget = 1;</script>',
      );
      const supported = symbolQuery(1, file, "unused");
      if (declaration) supported.symbol.col = 21;
      const response = run(root, [symbolQuery(0, "Widget.vue", "widget"), supported]);
      assert.deepEqual(
        response.results.map(({ assertion, status, reason_code }) => [
          assertion,
          status,
          reason_code,
        ]),
        [
          ["no-confirmed-use", "unavailable", "unsupported-syntax"],
          ["confirmed-no-static-references", "complete", null],
        ],
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

test("reused semantic sessions keep component queries isolated across revisions", async () => {
  const { parseRequest } = await import("../src/protocol.mjs");
  const { createSemanticSession } = await import("../src/semantic.mjs");
  const root = mkdtempSync(path.join(tmpdir(), "fallow-component-session-"));
  const session = createSemanticSession(root);
  try {
    writeFileSync(
      path.join(root, "tsconfig.json"),
      JSON.stringify({ files: ["api.ts", "main.ts"] }),
    );
    writeFileSync(path.join(root, "api.ts"), "export const used = 1;\n");
    writeFileSync(path.join(root, "main.ts"), 'import { used } from "./api";\nvoid used;\n');
    writeFileSync(
      path.join(root, "Widget.vue"),
      '<script lang="ts">export const widget = 1;</script>',
    );
    const request = (queries) =>
      parseRequest({
        protocol_version: WIRE_PROTOCOL_VERSION,
        operation: ANALYSIS_OPERATION,
        root,
        projects: ["tsconfig.json"],
        evidence_limit: 40,
        queries,
      });
    assert.equal(
      session.analyze(request([symbolQuery(1, "api.ts", "used")]), { revision: 1 }).results[0]
        .assertion,
      "confirmed-used",
    );
    const mixed = session.analyze(
      request([symbolQuery(0, "Widget.vue", "widget"), symbolQuery(1, "api.ts", "used")]),
      { revision: 2 },
    );
    assert.deepEqual(
      mixed.results.map(({ status, reasonCode }) => [status, reasonCode]),
      [
        ["unavailable", "unsupported-syntax"],
        ["complete", null],
      ],
    );
    assert.equal(mixed.results[1].assertion, "confirmed-used");
    const componentOnly = session.analyze(request([symbolQuery(0, "Widget.vue", "widget")]), {
      revision: 3,
    });
    assert.equal(componentOnly.results[0].reasonCode, "unsupported-syntax");
    assert.equal(
      session.analyze(request([symbolQuery(1, "api.ts", "used")]), { revision: 4 }).results[0]
        .assertion,
      "confirmed-used",
    );
  } finally {
    session.close();
    rmSync(root, { recursive: true, force: true });
  }
});
