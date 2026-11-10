import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { parseRequest } from "../src/protocol.mjs";
import { referenceNamesFromEdges } from "../src/reference-names.mjs";
import { analyzeSemanticQueries } from "../src/semantic.mjs";
import { ANALYSIS_OPERATION, WIRE_PROTOCOL_VERSION } from "../src/generated-protocol.mjs";

const HELPERS_SOURCE = "export const helper = (value: number): number => value;\n";

const FILES = {
  "src/helpers.ts": HELPERS_SOURCE,
  "src/barrel.ts": 'export { helper as aliased } from "./helpers";\n',
  "src/declared.d.ts": 'export { helper as declared } from "./helpers";\n',
  "src/renamed.ts":
    'import { helper as renamed } from "./helpers";\nexport const a = renamed(1);\n',
  "src/qualified.ts": 'import * as barrel from "./barrel";\nexport const b = barrel.aliased(2);\n',
  "src/namespace.ts": [
    'import * as helpers from "./helpers";',
    "export namespace Tools {",
    "  export import run = helpers.helper;",
    "}",
    "export const c = Tools.run(3);",
    "",
  ].join("\n"),
  "src/ambient.ts": 'import * as types from "./declared";\nexport const d = types.declared(4);\n',
  "src/chain.ts": 'export { aliased as twice } from "./barrel";\n',
  "src/chained.ts": 'import { twice as thrice } from "./chain";\nexport const e = thrice(5);\n',
  "src/unrelated.ts":
    'import { helper as unrelated } from "./other";\nexport const f = unrelated;\n',
  "src/other.ts": "export const helper = 1;\n",
};

const write = (root, relativePath, contents) => {
  const fileName = path.join(root, relativePath);
  mkdirSync(path.dirname(fileName), { recursive: true });
  writeFileSync(fileName, contents);
};

test("symbol-trace keeps uses that reach an export only through other names", () => {
  const root = mkdtempSync(path.join(tmpdir(), "fallow-type-aware-reference-names-"));
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    Object.entries(FILES).forEach(([file, contents]) => write(root, file, contents));
    const analysis = analyzeSemanticQueries(
      parseRequest({
        protocol_version: WIRE_PROTOCOL_VERSION,
        operation: ANALYSIS_OPERATION,
        root,
        projects: ["tsconfig.json"],
        evidence_limit: 40,
        queries: [
          {
            id: 0,
            operation: "symbol-trace",
            symbol: {
              path: "src/helpers.ts",
              namespace: "value",
              declaration_kind: "variable",
              exported_name: "helper",
              local_name: "helper",
              line: 1,
              col: HELPERS_SOURCE.indexOf("helper"),
            },
          },
        ],
      }),
    );

    assert.deepEqual(
      analysis.results[0].evidence.map((entry) => [entry.path, entry.role]).toSorted(),
      [
        ["src/ambient.ts", "property-access"],
        ["src/chained.ts", "call"],
        ["src/helpers.ts", "declaration"],
        ["src/namespace.ts", "property-access"],
        ["src/namespace.ts", "value-reference"],
        ["src/namespace.ts", "value-reference"],
        ["src/qualified.ts", "property-access"],
        ["src/renamed.ts", "call"],
      ],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("reference names follow alias chains and leave out unrelated imports", () => {
  const edges = [
    { from: "helper", to: "renamed" },
    { from: "renamed", to: "twice" },
    { from: "twice", to: "thrice" },
    { from: "useState", to: "useState" },
    { from: "path", to: "join" },
    { from: null, to: "React" },
    { from: null, to: "ns" },
  ];
  assert.deepEqual([...referenceNamesFromEdges(edges.toReversed(), ["helper"])].toSorted(), [
    "React",
    "default",
    "helper",
    "ns",
    "renamed",
    "thrice",
    "twice",
  ]);
});
