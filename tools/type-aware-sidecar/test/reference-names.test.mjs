import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
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

const traceHelperUses = (root, tsconfig, files, helperPath) => {
  write(root, "tsconfig.json", JSON.stringify(tsconfig));
  Object.entries(files).forEach(([file, contents]) => write(root, file, contents));
  const source = files[helperPath];
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
            path: helperPath,
            namespace: "value",
            declaration_kind: "variable",
            exported_name: "helper",
            local_name: "helper",
            line: 1,
            col: source.indexOf("helper"),
          },
        },
      ],
    }),
  );
  return analysis.results[0].evidence.map((entry) => [entry.path, entry.role]);
};

test("symbol-trace keeps uses that reach an export through a CommonJS export name", () => {
  const root = mkdtempSync(path.join(tmpdir(), "fallow-type-aware-reference-names-cjs-"));
  try {
    const uses = traceHelperUses(
      root,
      {
        compilerOptions: {
          allowJs: true,
          checkJs: false,
          module: "esnext",
          moduleResolution: "bundler",
          noEmit: true,
        },
        include: ["src/**/*"],
      },
      {
        "src/helpers.js": [
          "const helper = (value) => value;",
          "exports.run = helper;",
          "module.exports.go = helper;",
          'exports["byKey"] = helper;',
          "exports.first = helper, exports.second = helper;",
          'if (typeof exports === "object") { module.exports.nested = helper; }',
          "",
        ].join("\n"),
        "src/use.ts": [
          'import { run, go, byKey, first, second, nested } from "./helpers.js";',
          "run(1);",
          "go(2);",
          "byKey(3);",
          "first(4);",
          "second(5);",
          "nested(6);",
          "",
        ].join("\n"),
      },
      "src/helpers.js",
    );
    assert.deepEqual(
      uses.filter(([file]) => file === "src/use.ts"),
      Array.from({ length: 6 }, () => ["src/use.ts", "call"]),
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

for (const preserveSymlinks of [false, true]) {
  test(
    `symbol-trace keeps uses through a linked workspace package (preserveSymlinks: ${preserveSymlinks})`,
    { skip: process.platform === "win32" },
    () => {
      const root = realpathSync(
        mkdtempSync(path.join(tmpdir(), "fallow-type-aware-reference-names-link-")),
      );
      try {
        write(
          root,
          "packages/lib/package.json",
          JSON.stringify({ name: "@scope/lib", types: "index.ts" }),
        );
        mkdirSync(path.join(root, "node_modules/@scope"), { recursive: true });
        symlinkSync(path.join(root, "packages/lib"), path.join(root, "node_modules/@scope/lib"));
        const uses = traceHelperUses(
          root,
          {
            compilerOptions: {
              strict: true,
              preserveSymlinks,
              moduleResolution: "bundler",
              module: "esnext",
            },
            include: ["src/**/*.ts", "packages/lib/helpers.ts"],
          },
          {
            "packages/lib/helpers.ts": "export const helper = (value: number): number => value;\n",
            "packages/lib/index.ts": 'export { helper as run } from "./helpers";\n',
            "src/use.ts": 'import { run as go } from "@scope/lib";\ngo(1);\n',
          },
          "packages/lib/helpers.ts",
        );
        assert.ok(
          uses.some(([file, role]) => file === "src/use.ts" && role === "call"),
          `the call in src/use.ts must be a use, got: ${JSON.stringify(uses)}`,
        );
      } finally {
        rmSync(root, { recursive: true, force: true });
      }
    },
  );
}
