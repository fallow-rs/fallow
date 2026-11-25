import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { ANALYSIS_OPERATION, WIRE_PROTOCOL_VERSION } from "../src/generated-protocol.mjs";

// Issue https://github.com/fallow-rs/fallow/issues/4850: no file names
// `Entry`, but declaration emit of `game.ts` writes it, because the inferred
// type of `Game.latency` is `ObjectSchema<{ entry: Schema<Entry> }>`. Without
// the export of `Entry`, `tsc --declaration` fails with TS4029.

const sidecarRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const executable = process.env.FALLOW_TYPE_AWARE_BIN
  ? path.resolve(process.env.FALLOW_TYPE_AWARE_BIN)
  : path.join(sidecarRoot, "fallow-type-aware.mjs");

const EMIT_OPTIONS = { strict: true, declaration: true, emitDeclarationOnly: true };

// A library whose call result has a type that only the checker knows.
const SCHEMA_LIBRARY = [
  "export interface Schema<T> { readonly output: T }",
  "export interface ObjectSchema<T> { readonly shape: T }",
  "export declare function object<T>(shape: T): ObjectSchema<T>;",
  "export declare function string(): Schema<string>;",
  "",
].join("\n");

const SCHEMAS = [
  'import { object, string, type Schema } from "schema-lib";',
  "export interface Entry {",
  "  label: string;",
  "}",
  "export interface Unused {",
  "  nothing: string;",
  "}",
  "const entrySchema: Schema<Entry> = object({ label: string() }) as never;",
  "export const status = object({ entry: entrySchema });",
  "",
].join("\n");

const write = (root, relativePath, contents) => {
  const fileName = path.join(root, relativePath);
  mkdirSync(path.dirname(fileName), { recursive: true });
  writeFileSync(fileName, contents);
};

const typeQuery = (id, name, line) => ({
  id,
  operation: "symbol-use",
  symbol: {
    path: "src/schemas.ts",
    namespace: "type",
    declaration_kind: "export",
    exported_name: name,
    local_name: name,
    line,
    col: 17,
  },
  framework_contracts: [],
});

const runQueries = (root, queries) =>
  JSON.parse(
    execFileSync(executable, {
      cwd: sidecarRoot,
      input: JSON.stringify({
        protocol_version: WIRE_PROTOCOL_VERSION,
        operation: ANALYSIS_OPERATION,
        root,
        projects: ["tsconfig.json"],
        evidence_limit: 40,
        queries,
      }),
      encoding: "utf8",
    }),
  );

const assertions = (root) => {
  const response = JSON.parse(
    execFileSync(executable, {
      cwd: sidecarRoot,
      input: JSON.stringify({
        protocol_version: WIRE_PROTOCOL_VERSION,
        operation: ANALYSIS_OPERATION,
        root,
        projects: ["tsconfig.json"],
        evidence_limit: 40,
        queries: [typeQuery(0, "Entry", 2), typeQuery(1, "Unused", 5)],
      }),
      encoding: "utf8",
    }),
  );
  assert.equal(response.projects[0].status, "complete");
  const [entry, unused] = response.results;
  assert.equal(unused.assertion, "confirmed-no-static-references");
  return entry;
};

const withProject = ({ compilerOptions = EMIT_OPTIONS, packageJson = {}, files }, run) => {
  const root = mkdtempSync(path.join(tmpdir(), "fallow-type-aware-"));
  try {
    write(root, "package.json", JSON.stringify({ name: "emit", type: "module", ...packageJson }));
    write(
      root,
      "tsconfig.json",
      JSON.stringify({
        compilerOptions: { module: "esnext", moduleResolution: "bundler", ...compilerOptions },
        include: ["src/**/*.ts"],
      }),
    );
    write(
      root,
      "node_modules/schema-lib/package.json",
      '{ "name": "schema-lib", "types": "index.d.ts" }',
    );
    write(root, "node_modules/schema-lib/index.d.ts", SCHEMA_LIBRARY);
    write(root, "src/schemas.ts", SCHEMAS);
    Object.entries(files).forEach(([file, contents]) => write(root, file, contents));
    run(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

const gameWith = (member) =>
  `import { status } from "./schemas";\nexport class Game {\n  ${member} = status;\n}\n`;

test("credits a type that declaration emit of another file names", () => {
  withProject({ files: { "src/game.ts": gameWith("readonly latency") } }, (root) => {
    const entry = assertions(root);
    assert.equal(entry.assertion, "confirmed-used");
    assert.deepEqual(
      entry.evidence.map(({ path: file, line, role, namespace }) => ({
        file,
        line,
        role,
        namespace,
      })),
      [{ file: "src/game.ts", line: 3, role: "declaration-emit", namespace: "type" }],
    );
  });
});

test("credits a type that a protected member or an exported variable holds", () => {
  withProject({ files: { "src/game.ts": gameWith("protected latency") } }, (root) => {
    assert.equal(assertions(root).assertion, "confirmed-used");
  });
  const reExport = 'import { status } from "./schemas";\nconst held = status;\nexport { held };\n';
  withProject({ files: { "src/game.ts": reExport } }, (root) => {
    assert.equal(assertions(root).assertion, "confirmed-used");
  });
  const throughBarrel = {
    "src/index.ts": 'export * from "./schemas";\n',
    "src/game.ts": gameWith("readonly latency").replace('"./schemas"', '"./index"'),
  };
  withProject({ files: throughBarrel }, (root) => {
    assert.equal(assertions(root).assertion, "confirmed-used");
  });
});

test("credits a type for a package that declares its types without declaration", () => {
  withProject(
    {
      compilerOptions: { strict: true },
      packageJson: { exports: { ".": { types: "./dist/game.d.ts", default: "./dist/game.js" } } },
      files: { "src/game.ts": gameWith("readonly latency") },
    },
    (root) => assert.equal(assertions(root).assertion, "confirmed-used"),
  );
});

test("does not credit a type that declaration emit does not need", () => {
  const cases = {
    "a private member, which emit writes without a type": {
      files: { "src/game.ts": gameWith("private latency") },
    },
    "a holder in the file that declares the type": {
      files: { "src/game.ts": "export const unrelated = 1;\n" },
    },
    "a project that emits no declaration files": {
      compilerOptions: { strict: true },
      files: { "src/game.ts": gameWith("readonly latency") },
    },
    "an exported alias, which emit names instead of its structure": {
      files: {
        "src/schemas.ts": `${SCHEMAS}export type EntrySchema = Schema<Entry>;\n`,
        "src/game.ts": [
          'import { object } from "schema-lib";',
          'import type { EntrySchema } from "./schemas";',
          "declare const schema: EntrySchema;",
          "export const wrapped = object({ schema });",
          "",
        ].join("\n"),
      },
    },
    "an annotation, which names the type in the source": {
      files: {
        "src/game.ts": [
          'import type { ObjectSchema } from "schema-lib";',
          'import { status } from "./schemas";',
          "export const held: ObjectSchema<unknown> = status;",
          "",
        ].join("\n"),
      },
    },
  };
  for (const [name, project] of Object.entries(cases)) {
    withProject(project, (root) => {
      assert.notEqual(assertions(root).assertion, "confirmed-used", name);
    });
  }
});

test("does not credit a type inside a value that emit writes as `typeof`", () => {
  const hidden = [
    "export interface Hidden {",
    "  h: string;",
    "}",
    "export function g() {",
    '  const value: Hidden = { h: "a" };',
    "  return value;",
    "}",
    "",
  ].join("\n");
  const files = {
    "src/schemas.ts": `${SCHEMAS}${hidden}`,
    "src/game.ts": 'import { g, status } from "./schemas";\nexport const obj = { status, f: g };\n',
  };
  withProject({ files }, (root) => {
    const hiddenLine = SCHEMAS.split("\n").length;
    const [entry, hiddenResult] = runQueries(root, [
      typeQuery(0, "Entry", 2),
      typeQuery(1, "Hidden", hiddenLine),
    ]).results;
    assert.equal(entry.assertion, "confirmed-used");
    assert.notEqual(hiddenResult.assertion, "confirmed-used");
  });
});
