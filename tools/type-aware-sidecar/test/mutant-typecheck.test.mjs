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

const CHAIN_SOURCE = [
  "export const member = (m1: { b: number }): number => m1?.b ?? 0;",
  "export const call = (c1: () => number): number => c1?.() ?? 0;",
  'export const element = (e1: { b: number }): number => e1?.["b"] ?? 0;',
  "export const maybe = (u1: { b: number } | undefined): number => u1?.b ?? 0;",
  "export const indexed = (xs: { b: number }[]): number => xs[0]?.b ?? 0;",
  "export const loose = (l1: any): number => (l1 as any)?.b ?? 0;",
  "export const cast = (k1: unknown): number => (k1 as { b: number })?.b ?? 0;",
  "export const asserted = (n1: { b: number } | null): number => n1!?.b ?? 0;",
  "export const narrowed = (w1: { b: number } | undefined): number => (w1 ? w1?.b : 0);",
  "export const generic = <T extends { b: number }>(g1: T): number => g1?.b ?? 0;",
  'export const wide = "é€😀"; export const later = (z1: { b: number }): number => z1?.b ?? 0;',
  "export const spaced = (s1: { b: number }): number => s1",
  "  ?.b ?? 0;",
  "",
].join("\n");

const linkAfter = (source, id, anchor, replacement) => {
  const index = source.indexOf(anchor) + anchor.length;
  assert.equal(source.slice(index, index + 2), "?.", `${id} anchor`);
  const start = Buffer.byteLength(source.slice(0, index), "utf8");
  return { id, start, end: start + 2, replacement };
};

const chainMutants = (source) => [
  linkAfter(source, "member", "=> m1", "."),
  linkAfter(source, "call", "=> c1", ""),
  linkAfter(source, "element", "=> e1", ""),
  linkAfter(source, "maybe", "=> u1", "."),
  linkAfter(source, "indexed", "=> xs[0]", "."),
  linkAfter(source, "loose", "(l1 as any)", "."),
  linkAfter(source, "cast", "(k1 as { b: number })", "."),
  linkAfter(source, "asserted", "=> n1!", "."),
  linkAfter(source, "narrowed", "? w1", "."),
  linkAfter(source, "generic", "=> g1", "."),
  linkAfter(source, "later", "=> z1", "."),
  linkAfter(source, "spaced", "=> s1\n  ", "."),
];

const TYPE_PROVEN = "type-proven-never-nullish";

const checkChains = (compilerOptions) => {
  const root = makeProject({ "src/chain.ts": CHAIN_SOURCE }, compilerOptions);
  try {
    const parsed = parseRequest(
      request(root, [
        {
          path: "src/chain.ts",
          source_sha256: sha256(CHAIN_SOURCE),
          mutants: chainMutants(CHAIN_SOURCE),
        },
      ]),
    );
    return Object.fromEntries(checkMutants(parsed).map((result) => [result.id, result]));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

const equivalent = (id) => ({ id, outcome: "equivalent", reason: TYPE_PROVEN });

test("an optional link on a type that is never nullish is equivalent", () => {
  const results = checkChains({ noUncheckedIndexedAccess: true });
  assert.deepEqual(results.member, equivalent("member"));
  assert.deepEqual(results.call, equivalent("call"));
  assert.deepEqual(results.element, equivalent("element"));
  assert.deepEqual(results.later, equivalent("later"));
  assert.deepEqual(results.spaced, equivalent("spaced"));
});

test("an optional link whose receiver may be nullish or is unsound keeps the normal check", () => {
  const results = checkChains({ noUncheckedIndexedAccess: true });
  assert.equal(results.maybe.outcome, "compile-error");
  assert.equal(results.indexed.outcome, "compile-error");
  for (const id of ["loose", "cast", "asserted", "generic", "narrowed"]) {
    assert.deepEqual(results[id], { id, outcome: "compiles" }, id);
  }
});

test("the proof needs strictNullChecks and noUncheckedIndexedAccess", () => {
  const settings = [
    { noUncheckedIndexedAccess: false },
    { noUncheckedIndexedAccess: undefined },
    { strict: false, noUncheckedIndexedAccess: true },
    { strictNullChecks: false, noUncheckedIndexedAccess: true },
  ];
  for (const compilerOptions of settings) {
    const results = checkChains(compilerOptions);
    assert.equal(results.member.outcome, "compiles", JSON.stringify(compilerOptions));
    assert.equal(results.later.outcome, "compiles", JSON.stringify(compilerOptions));
  }
  const explicit = checkChains({
    strict: false,
    strictNullChecks: true,
    noUncheckedIndexedAccess: true,
  });
  assert.deepEqual(explicit.member, equivalent("member"));
});

test("a JavaScript file is never proven equivalent", () => {
  const javascript = "export const get = (a) => a?.b;\n";
  const root = makeProject(
    { "src/get.js": javascript },
    { allowJs: true, checkJs: true, noUncheckedIndexedAccess: true },
  );
  try {
    const parsed = parseRequest(
      request(root, [{ path: "src/get.js", mutants: [linkAfter(javascript, "js", "=> a", ".")] }]),
    );
    assert.deepEqual(checkMutants(parsed), [{ id: "js", outcome: "compiles" }]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

const RISK_SOURCE = [
  'import { cfg } from "./cfg";',
  'import { amb } from "./ambient";',
  "declare const supplied: { b: number };",
  "declare let suppliedLet: { b: number };",
  "function getB(): { b: number } { return { b: 1 }; }",
  "function inferredB() { return { b: 1 }; }",
  "export class Box {",
  "  value!: { b: number };",
  "  unset: { b: number };",
  "  declare ambient: { b: number };",
  "  set: { b: number } = { b: 1 };",
  "  assigned: { b: number };",
  "  constructor(readonly param: { b: number }) { this.assigned = param; }",
  "  readValue() { return this.value?.b ?? 0; }",
  "  readUnset() { return this.unset?.b ?? 0; }",
  "  readAmbient() { return this.ambient?.b ?? 0; }",
  "  readSet() { return this.set?.b ?? 0; }",
  "  readAssigned() { return this.assigned?.b ?? 0; }",
  "  readParam() { return this.param?.b ?? 0; }",
  "}",
  "export const nested = (n0: unknown): number => (n0 as { child: { b: number } }).child?.b ?? 0;",
  "export const aliased = (n1: unknown): number => { const aliasedX = n1 as { b: number }; return aliasedX?.b ?? 0; };",
  "export const chained = (n2: unknown): number => { const c1 = n2 as { b: number }; const chainedY = c1; return chainedY?.b ?? 0; };",
  "export const angle = (n3: unknown): number => { const angleX = <{ b: number }>n3; return angleX?.b ?? 0; };",
  "export const bang = (n4: { b: number } | null): number => { const bangX = n4!; return bangX?.b ?? 0; };",
  "export const viaSatisfies = (n5: { b: number }): number => (n5 satisfies { b: number })?.b ?? 0;",
  "export const ambientVar = (): number => supplied?.b ?? 0;",
  "export const ambientLet = (): number => suppliedLet?.b ?? 0;",
  "export const ambientFile = (): number => amb?.b ?? 0;",
  "export const imported = (): number => cfg?.b ?? 0;",
  "export const literalLocal = (): number => { const o = { b: 1 }; return o?.b ?? 0; };",
  "export const annotatedCall = (): number => getB()?.b ?? 0;",
  "export const inferredCall = (): number => inferredB()?.b ?? 0;",
  "export const viaParam = (box: Box): number => box.set?.b ?? 0;",
  "export const stale = (): number | undefined => {",
  "  let a: { b: number } | undefined = { b: 1 };",
  "  const reset = (): void => { a = undefined; };",
  "  if (a) { reset(); return a?.b; }",
  "  return 0;",
  "};",
  "export const staleAlias = (): number | undefined => {",
  "  let s: { b: number } | undefined = { b: 1 };",
  "  const clear = (): void => { s = undefined; };",
  "  if (s) { clear(); const staleY = s; return staleY?.b; }",
  "  return 0;",
  "};",
  "",
].join("\n");

const riskMutants = (source) => [
  linkAfter(source, "value", "return this.value", "."),
  linkAfter(source, "unset", "return this.unset", "."),
  linkAfter(source, "ambient", "return this.ambient", "."),
  linkAfter(source, "set", "return this.set", "."),
  linkAfter(source, "assigned", "return this.assigned", "."),
  linkAfter(source, "param", "return this.param", "."),
  linkAfter(source, "nested", ").child", "."),
  linkAfter(source, "aliased", "return aliasedX", "."),
  linkAfter(source, "chained", "return chainedY", "."),
  linkAfter(source, "angle", "return angleX", "."),
  linkAfter(source, "bang", "return bangX", "."),
  linkAfter(source, "viaSatisfies", "(n5 satisfies { b: number })", "."),
  linkAfter(source, "ambientVar", "=> supplied", "."),
  linkAfter(source, "ambientLet", "=> suppliedLet", "."),
  linkAfter(source, "ambientFile", "=> amb", "."),
  linkAfter(source, "imported", "=> cfg", "."),
  linkAfter(source, "literalLocal", "return o", "."),
  linkAfter(source, "annotatedCall", "=> getB()", "."),
  linkAfter(source, "inferredCall", "=> inferredB()", "."),
  linkAfter(source, "viaParam", "=> box.set", "."),
  linkAfter(source, "stale", "reset(); return a", "."),
  linkAfter(source, "staleAlias", "return staleY", "."),
];

const checkRisks = () => {
  const root = makeProject(
    {
      "src/risk.ts": RISK_SOURCE,
      "src/cfg.ts": "export const cfg = { b: 1 };\n",
      "src/ambient.d.ts": "export declare const amb: { b: number };\n",
    },
    { noUncheckedIndexedAccess: true, strictPropertyInitialization: false },
  );
  try {
    const parsed = parseRequest(
      request(root, [
        {
          path: "src/risk.ts",
          source_sha256: sha256(RISK_SOURCE),
          mutants: riskMutants(RISK_SOURCE),
        },
      ]),
    );
    return Object.fromEntries(checkMutants(parsed).map((result) => [result.id, result]));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

test("a cast, an alias of a cast or a non-null assertion is never proven", () => {
  const results = checkRisks();
  for (const id of ["nested", "aliased", "chained", "angle", "bang"]) {
    assert.deepEqual(results[id], { id, outcome: "compiles" }, id);
  }
});

test("a declaration that nothing forces to hold a value is never proven", () => {
  const results = checkRisks();
  for (const id of ["value", "unset", "ambient", "ambientVar", "ambientLet", "ambientFile"]) {
    assert.deepEqual(results[id], { id, outcome: "compiles" }, id);
  }
});

test("a narrowing that a call can invalidate is never proven", () => {
  const results = checkRisks();
  for (const id of ["stale", "staleAlias"]) {
    assert.deepEqual(results[id], { id, outcome: "compiles" }, id);
  }
});

test("declared types that the language enforces still prove a receiver", () => {
  const results = checkRisks();
  for (const id of [
    "set",
    "assigned",
    "param",
    "viaSatisfies",
    "imported",
    "literalLocal",
    "annotatedCall",
    "viaParam",
  ]) {
    assert.deepEqual(results[id], equivalent(id), id);
  }
  assert.deepEqual(results.inferredCall, { id: "inferredCall", outcome: "compiles" });
});
