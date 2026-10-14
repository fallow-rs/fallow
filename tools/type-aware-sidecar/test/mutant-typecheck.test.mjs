import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { checkMutants } from "../src/mutant-typecheck.mjs";
import { parseRequest } from "../src/protocol.mjs";
import { MUTANT_TYPECHECK_OPERATION, WIRE_PROTOCOL_VERSION } from "../src/generated-protocol.mjs";

const sidecarRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const executable = process.env.FALLOW_TYPE_AWARE_BIN
  ? path.resolve(process.env.FALLOW_TYPE_AWARE_BIN)
  : path.join(sidecarRoot, "fallow-type-aware.mjs");

const SOURCE = [
  "export const add = (a: number, b: number): number => a + b;",
  "export const label = (n: number): string => `n=${n}`;",
  "export const isBig = (n: number): boolean => n > 10;",
  "",
].join("\n");

const write = (root, relativePath, contents) => {
  const fileName = path.join(root, relativePath);
  mkdirSync(path.dirname(fileName), { recursive: true });
  writeFileSync(fileName, contents);
};

const makeProject = (files, compilerOptions = {}) => {
  const root = mkdtempSync(path.join(tmpdir(), "fallow-mutant-typecheck-"));
  write(
    root,
    "tsconfig.json",
    JSON.stringify({
      compilerOptions: { strict: true, noEmit: true, ...compilerOptions },
      include: ["src"],
    }),
  );
  for (const [relativePath, contents] of Object.entries(files)) write(root, relativePath, contents);
  return root;
};

const mutantAt = (source, id, original, replacement) => {
  const start = Buffer.byteLength(source.slice(0, source.indexOf(original)), "utf8");
  return { id, start, end: start + Buffer.byteLength(original, "utf8"), replacement };
};

const request = (root, files) => ({
  protocol_version: WIRE_PROTOCOL_VERSION,
  operation: MUTANT_TYPECHECK_OPERATION,
  root,
  files,
});

const sha256 = (text) => createHash("sha256").update(text).digest("hex");

const sourceMutants = () => [
  mutantAt(SOURCE, "arith", "a + b", "a - b"),
  mutantAt(SOURCE, "string", "`n=${n}`", "0"),
  mutantAt(SOURCE, "relational", "n > 10", '""'),
];

test("a mutant with a new type error is a compile error, a valid mutant compiles", () => {
  const root = makeProject({ "src/math.ts": SOURCE });
  try {
    const parsed = parseRequest(
      request(root, [
        { path: "src/math.ts", source_sha256: sha256(SOURCE), mutants: sourceMutants() },
      ]),
    );
    assert.deepEqual(checkMutants(parsed), [
      { id: "arith", outcome: "compiles" },
      { id: "string", outcome: "compile-error", codes: [2322] },
      { id: "relational", outcome: "compile-error", codes: [2322] },
    ]);
    assert.equal(readFileSync(path.join(root, "src/math.ts"), "utf8"), SOURCE);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("an error that the original file already has does not make a compile error", () => {
  const broken = `${SOURCE}export const wrong: string = 1;\n`;
  const root = makeProject({ "src/math.ts": broken });
  try {
    const parsed = parseRequest(
      request(root, [
        {
          path: "src/math.ts",
          mutants: [mutantAt(broken, "arith", "a + b", "a - b")],
        },
      ]),
    );
    assert.deepEqual(checkMutants(parsed), [{ id: "arith", outcome: "compiles" }]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("files that the checker does not check stay unchecked", () => {
  const noCheck = `// @ts-nocheck\n${SOURCE}`;
  const javascript = "export const label = (n) => `n=${n}`;\n";
  const root = makeProject(
    { "src/nocheck.ts": noCheck, "src/plain.js": javascript, "src/math.ts": SOURCE },
    { allowJs: true },
  );
  write(root, "outside/loose.ts", SOURCE);
  try {
    const parsed = parseRequest(
      request(root, [
        { path: "src/nocheck.ts", mutants: [mutantAt(noCheck, "nocheck", "`n=${n}`", "0")] },
        { path: "src/plain.js", mutants: [mutantAt(javascript, "js", "`n=${n}`", "0")] },
        { path: "outside/loose.ts", mutants: [mutantAt(SOURCE, "loose", "`n=${n}`", "0")] },
        {
          path: "src/math.ts",
          source_sha256: sha256("other text"),
          mutants: [mutantAt(SOURCE, "changed", "`n=${n}`", "0")],
        },
        { path: "src/gone.ts", mutants: [{ id: "gone", start: 0, end: 1, replacement: "x" }] },
      ]),
    );
    assert.deepEqual(checkMutants(parsed), [
      { id: "nocheck", outcome: "unchecked", reason: "not-type-checked" },
      { id: "js", outcome: "unchecked", reason: "not-type-checked" },
      { id: "loose", outcome: "unchecked", reason: "no-project" },
      { id: "changed", outcome: "unchecked", reason: "source-changed" },
      { id: "gone", outcome: "unchecked", reason: "file-missing" },
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the request parser refuses invalid mutant requests", () => {
  const root = makeProject({});
  try {
    const file = (mutants) => request(root, [{ path: "src/a.ts", mutants }]);
    assert.throws(
      () =>
        parseRequest(
          file([
            { id: "m", start: 0, end: 1, replacement: "" },
            { id: "m", start: 0, end: 1, replacement: "" },
          ]),
        ),
      /duplicate mutant id m/u,
    );
    assert.throws(
      () => parseRequest(file([{ id: "m", start: 4, end: 1, replacement: "" }])),
      /end must be an integer greater than or equal to 4/u,
    );
    assert.throws(
      () => parseRequest(request(root, [{ path: "../a.ts", mutants: [] }])),
      /must resolve within root/u,
    );
    assert.throws(
      () => parseRequest({ ...request(root, []), queries: [] }),
      /unknown field queries/u,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the executable answers a one-shot mutant-typecheck request", () => {
  const root = makeProject({ "src/math.ts": SOURCE });
  try {
    const result = spawnSync(executable, [], {
      input: JSON.stringify(
        request(root, [{ path: "src/math.ts", mutants: sourceMutants().slice(0, 2) }]),
      ),
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    const response = JSON.parse(result.stdout);
    assert.equal(response.operation, MUTANT_TYPECHECK_OPERATION);
    assert.equal(response.protocol_version, WIRE_PROTOCOL_VERSION);
    assert.deepEqual(response.results, [
      { id: "arith", outcome: "compiles" },
      { id: "string", outcome: "compile-error", codes: [2322] },
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
