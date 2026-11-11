import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { collectCallEvidence } from "../src/call-evidence.mjs";
import { parseRequest } from "../src/protocol.mjs";
import { CALL_EVIDENCE_OPERATION, WIRE_PROTOCOL_VERSION } from "../src/generated-protocol.mjs";

const sidecarRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const executable = process.env.FALLOW_TYPE_AWARE_BIN
  ? path.resolve(process.env.FALLOW_TYPE_AWARE_BIN)
  : path.join(sidecarRoot, "fallow-type-aware.mjs");

const write = (root, relativePath, contents) => {
  const fileName = path.join(root, relativePath);
  mkdirSync(path.dirname(fileName), { recursive: true });
  writeFileSync(fileName, contents);
};

const makeProject = (files, compilerOptions = { strict: true }) => {
  const root = mkdtempSync(path.join(tmpdir(), "fallow-call-evidence-"));
  write(
    root,
    "tsconfig.json",
    JSON.stringify({
      compilerOptions: { noEmit: true, target: "es2022", ...compilerOptions },
      include: ["src"],
    }),
  );
  for (const [relativePath, contents] of Object.entries(files)) write(root, relativePath, contents);
  return root;
};

const withProject = (files, compilerOptions, run) => {
  const root = makeProject(files, compilerOptions);
  try {
    return run(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

// The byte offset of each call of `callee` in `source`, in source order.
const callStarts = (source, callee) => {
  const starts = [];
  for (
    let at = source.indexOf(`${callee}(`);
    at !== -1;
    at = source.indexOf(`${callee}(`, at + 1)
  ) {
    const before = source[at - 1];
    if (before !== undefined && /[\w$.]/u.test(before)) continue;
    starts.push(Buffer.byteLength(source.slice(0, at), "utf8"));
  }
  return starts;
};

const defaultOutcome = (files, compilerOptions, file, callee, index) =>
  withProject(files, compilerOptions, (root) => {
    const starts = callStarts(files[file], callee);
    const { defaults } = collectCallEvidence({
      root,
      defaults: [
        { id: 0, arguments: starts.map((start) => ({ path: file, call_start: start, index })) },
      ],
      classes: [],
      files: Object.keys(files),
    });
    return defaults[0];
  });

const methodResults = (files, classes, compilerOptions = { strict: true }) =>
  withProject(
    files,
    compilerOptions,
    (root) =>
      collectCallEvidence({ root, defaults: [], classes, files: Object.keys(files) }).methods,
  );

const FUNCTION =
  "const fmt = (value: string, width: number = 2): string => value.padStart(width);\n";

test("a default is defined when each argument excludes undefined", () => {
  const source = `${FUNCTION}const w = 4;\nexport const run = (n: number) => [fmt("a", w), fmt("b", n), fmt("c", 3)];\n`;
  const result = defaultOutcome({ "src/a.ts": source }, undefined, "src/a.ts", "fmt", 1);
  assert.deepEqual(result, { id: 0, outcome: "defined" });
});

test("an argument that can be undefined keeps the default", () => {
  const source = `${FUNCTION}export const run = (n?: number) => [fmt("a", n), fmt("b", 3)];\n`;
  const result = defaultOutcome({ "src/a.ts": source }, undefined, "src/a.ts", "fmt", 1);
  assert.equal(result.outcome, "maybe-undefined");
});

test("a binding that a closure writes uses its declared type", () => {
  const source = [
    "const pick = (chunks: string[], partial: boolean | undefined = false) => (partial ? chunks : []);",
    "export const run = (later: (cb: () => void) => void) => {",
    "  let partial: boolean | undefined = false;",
    "  later(() => { partial = undefined; });",
    "  pick([], partial);",
    "  pick([], true);",
    "};",
    "",
  ].join("\n");
  const result = defaultOutcome({ "src/a.ts": source }, undefined, "src/a.ts", "pick", 1);
  assert.notEqual(result.outcome, "defined");
});

test("an index read is not evidence without noUncheckedIndexedAccess", () => {
  const source = `${FUNCTION}export const run = (widths: number[]) => {\n  const [first] = widths;\n  const second = widths[1];\n  return [fmt("a", first), fmt("b", second), fmt("c", widths[2])];\n};\n`;
  const loose = defaultOutcome({ "src/a.ts": source }, undefined, "src/a.ts", "fmt", 1);
  assert.notEqual(loose.outcome, "defined");
  const checked = defaultOutcome(
    { "src/a.ts": source },
    { strict: true, noUncheckedIndexedAccess: true },
    "src/a.ts",
    "fmt",
    1,
  );
  assert.notEqual(checked.outcome, "defined");
});

test("a class field that can stay unset is not evidence", () => {
  const source = `${FUNCTION}export class Box {\n  width!: number;\n  run() { return [fmt("a", this.width), fmt("b", 1)]; }\n}\n`;
  const result = defaultOutcome({ "src/a.ts": source }, undefined, "src/a.ts", "fmt", 1);
  assert.notEqual(result.outcome, "defined");
});

test("an assertion or a project without strictNullChecks gives no evidence", () => {
  const cast = `${FUNCTION}export const run = (n: unknown) => [fmt("a", n as number), fmt("b", 1)];\n`;
  assert.notEqual(
    defaultOutcome({ "src/a.ts": cast }, undefined, "src/a.ts", "fmt", 1).outcome,
    "defined",
  );
  const plain = `${FUNCTION}export const run = (n: number) => [fmt("a", n), fmt("b", n)];\n`;
  assert.notEqual(
    defaultOutcome({ "src/a.ts": plain }, { strict: false }, "src/a.ts", "fmt", 1).outcome,
    "defined",
  );
});

const SERVICE = [
  "export interface Saver { save(id: string, retries?: number): void }",
  "export class Service implements Saver {",
  "  save(id: string, retries: number = 3): void { void id; void retries; }",
  "  load(id: string, cache?: boolean): string { return cache ? id : id; }",
  "}",
  "export class Sub extends Service {}",
  "",
].join("\n");

const CLASSES = [{ id: 0, path: "src/svc.ts", class: "Service", methods: ["save", "load"] }];

test("method calls through the class, a subclass and an interface are all visible", () => {
  const use = [
    'import { Service, Sub, type Saver } from "./svc";',
    "const s = new Service();",
    "const saver: Saver = s;",
    's.save("a", 1); new Sub().save("b", 2); saver.save("c", 3);',
    's.load("a"); s.load("b");',
    "",
  ].join("\n");
  const [save, load] = methodResults({ "src/svc.ts": SERVICE, "src/use.ts": use }, CLASSES);
  assert.equal(save.outcome, "complete");
  assert.equal(save.calls.length, 3);
  assert.ok(save.calls.every((call) => call.args[1].maybe_undefined === false));
  assert.ok(save.calls.every((call) => !path.isAbsolute(call.path)));
  assert.equal(load.outcome, "complete");
  assert.ok(load.calls.every((call) => call.arg_count === 1));
});

test("a structurally compatible type that the class does not implement blocks the method", () => {
  const use = [
    'import { Service } from "./svc";',
    "type Loader = { load(id: string, cache?: boolean): string };",
    'const viaShape = (loader: Loader) => loader.load("x", true);',
    'const s = new Service(); s.load("a"); s.load("b"); viaShape(s);',
    "",
  ].join("\n");
  const [, load] = methodResults({ "src/svc.ts": SERVICE, "src/use.ts": use }, CLASSES);
  assert.deepEqual([load.outcome, load.reason], ["abstained", "structural-alias"]);
});

test("a value reference, an untyped receiver or a computed access blocks the method", () => {
  const value =
    'import { Service } from "./svc";\nconst s = new Service(); s.load("a"); s.load("b"); const f = s.load;\nvoid f;\n';
  assert.equal(
    methodResults({ "src/svc.ts": SERVICE, "src/use.ts": value }, CLASSES)[1].reason,
    "value-reference",
  );
  const untyped =
    'import { Service } from "./svc";\nconst s = new Service(); s.load("a"); s.load("b");\nexport const x = (o: any) => o.load("c", true);\n';
  assert.equal(
    methodResults({ "src/svc.ts": SERVICE, "src/use.ts": untyped }, CLASSES)[1].reason,
    "unresolved-reference",
  );
  const computed =
    'import { Service } from "./svc";\nconst s = new Service(); s.load("a"); s.load("b");\nexport const x = (k: "load" | "save") => s[k]("c");\n';
  assert.equal(
    methodResults({ "src/svc.ts": SERVICE, "src/use.ts": computed }, CLASSES)[1].reason,
    "dynamic-member-access",
  );
});

test("a file outside the program that names the method blocks it", () => {
  const use =
    'import { Service } from "./svc";\nconst s = new Service(); s.load("a"); s.load("b");\n';
  const [, load] = methodResults(
    {
      "src/svc.ts": SERVICE,
      "src/use.ts": use,
      "src/View.vue": "<template>{{ svc.load('x', true) }}</template>\n",
    },
    CLASSES,
  );
  assert.deepEqual([load.outcome, load.reason], ["abstained", "outside-program"]);
});

test("a decorator or a member of an external base type blocks the method", () => {
  const decorated = SERVICE.replace(
    "export class Service",
    "const mark = (c: unknown, _: unknown) => c;\n@mark\nexport class Service",
  );
  const use =
    'import { Service } from "./svc";\nconst s = new Service(); s.load("a"); s.load("b");\n';
  assert.equal(
    methodResults({ "src/svc.ts": decorated, "src/use.ts": use }, CLASSES, {
      strict: true,
      experimentalDecorators: false,
    })[1].reason,
    "decorator",
  );
  const external = [
    'import { Base } from "lib";',
    "export class Service extends Base {",
    "  load(id: string, cache?: boolean): string { return cache ? id : id; }",
    "}",
    "",
  ].join("\n");
  const [result] = methodResults(
    {
      "src/svc.ts": external,
      "src/use.ts": use,
      "node_modules/lib/package.json": '{"name":"lib","types":"index.d.ts"}',
      "node_modules/lib/index.d.ts":
        "export declare class Base { load(id: string, cache?: boolean): string }\n",
    },
    [{ id: 0, path: "src/svc.ts", class: "Service", methods: ["load"] }],
    { strict: true, moduleResolution: "bundler", module: "esnext" },
  );
  assert.deepEqual([result.outcome, result.reason], ["abstained", "external-contract"]);
});

test("the request parser rejects unknown fields and paths outside the root", () => {
  const root = mkdtempSync(path.join(tmpdir(), "fallow-call-evidence-"));
  try {
    const base = {
      protocol_version: WIRE_PROTOCOL_VERSION,
      operation: CALL_EVIDENCE_OPERATION,
      root,
      defaults: [],
      classes: [],
      files: [],
    };
    assert.equal(parseRequest(base).operation, CALL_EVIDENCE_OPERATION);
    assert.throws(() => parseRequest({ ...base, queries: [] }), /unknown field queries/u);
    assert.throws(() => parseRequest({ ...base, files: ["../outside.ts"] }), /within root/u);
    assert.throws(
      () =>
        parseRequest({
          ...base,
          defaults: [
            { id: 0, arguments: [] },
            { id: 0, arguments: [] },
          ],
        }),
      /duplicate default id/u,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the executable answers a one-shot call-evidence request", () => {
  const source = `${FUNCTION}export const run = (n: number) => [fmt("a", n), fmt("b", 3)];\n`;
  withProject({ "src/a.ts": source }, undefined, (root) => {
    const starts = callStarts(source, "fmt");
    const result = spawnSync(executable, [], {
      input: JSON.stringify({
        protocol_version: WIRE_PROTOCOL_VERSION,
        operation: CALL_EVIDENCE_OPERATION,
        root,
        defaults: [{ id: 7, arguments: [{ path: "src/a.ts", call_start: starts[0], index: 1 }] }],
        classes: [],
        files: ["src/a.ts"],
      }),
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    const response = JSON.parse(result.stdout);
    assert.equal(response.operation, CALL_EVIDENCE_OPERATION);
    assert.deepEqual(response.defaults, [{ id: 7, outcome: "defined" }]);
    assert.deepEqual(response.methods, []);
  });
});

const UNSOUND_DEFAULTS = {
  "the tail of an optional chain": `${FUNCTION}type B = { c: number };\ntype A = { b: B };\nexport const run = (a?: A) => [fmt("x", a?.b.c), fmt("y", 1)];\n`,
  "an assertion one binding away": `${FUNCTION}export const run = (n: unknown) => {\n  const a = n as number;\n  const b = a;\n  return [fmt("x", b), fmt("y", 1)];\n};\n`,
  "an index read one binding away": `${FUNCTION}export const run = (xs: number[]) => {\n  const a = xs[5];\n  const b = a;\n  return [fmt("x", b), fmt("y", 1)];\n};\n`,
  "an index read in an object literal member": `${FUNCTION}export const run = (xs: number[]) => {\n  const o = { w: xs[5] };\n  return [fmt("x", o.w), fmt("y", 1)];\n};\n`,
  "an index read in a helper": `${FUNCTION}const first = (xs: number[]) => xs[0];\nexport const run = (xs: number[]) => [fmt("x", first(xs)), fmt("y", 1)];\n`,
  "a ts-expect-error comment": `${FUNCTION}// @ts-expect-error\nconst w: number = undefined;\nexport const run = () => [fmt("x", w), fmt("y", 1)];\n`,
  "a ts-nocheck comment": `// @ts-nocheck\n${FUNCTION}const w: number = undefined;\nexport const run = () => [fmt("x", w), fmt("y", 1)];\n`,
  "an any initializer": `${FUNCTION}const w: number = JSON.parse("null");\nexport const run = () => [fmt("x", w), fmt("y", 1)];\n`,
  "a type predicate": `${FUNCTION}const isNum = (v: unknown): v is number => true;\nexport const run = (n?: number) => (isNum(n) ? [fmt("x", n), fmt("y", 1)] : []);\n`,
  "a field read in the constructor": `${FUNCTION}export class Box {\n  w: number;\n  constructor() {\n    fmt("x", this.w);\n    fmt("y", 1);\n    this.w = 1;\n  }\n}\n`,
};

for (const [name, source] of Object.entries(UNSOUND_DEFAULTS)) {
  test(`${name} is not evidence that a value is defined`, () => {
    const result = defaultOutcome({ "src/a.ts": source }, undefined, "src/a.ts", "fmt", 1);
    assert.notEqual(result.outcome, "defined");
  });
}

const STORE =
  "export class Service {\n  load(id: string, cache?: boolean): string { return cache ? id : id; }\n}\n";
const STORE_USE =
  'import { Service } from "./svc";\nexport const s = new Service();\ns.load("a");\ns.load("b");\n';
const LOAD = [{ id: 0, path: "src/svc.ts", class: "Service", methods: ["load"] }];

const HIDDEN_CALLS = {
  "a computed read in a file without the name": [
    {
      "src/other.ts":
        'import { s } from "./use";\nimport type { Service } from "./svc";\nexport const x = (k: keyof Service) => {\n  const f = s[k];\n  return f.call(s, "c", true);\n};\n',
    },
    "dynamic-member-access",
  ],
  "a computed call on a call result": [
    {
      "src/other.ts":
        'import { s } from "./use";\nimport type { Service } from "./svc";\nconst get = () => s;\nexport const x = (k: keyof Service) => get()[k]("c", true);\n',
    },
    "dynamic-member-access",
  ],
  "a computed call on an asserted receiver": [
    {
      "src/other.ts":
        'import type { Service } from "./svc";\nexport const x = (k: keyof Service, t: unknown) => (t as Service)[k]("c", true);\n',
    },
    "dynamic-member-access",
  ],
  "a loop over the prototype": [
    {
      "src/other.ts":
        'import { s } from "./use";\nimport { Service } from "./svc";\nconst anyS: any = s;\nfor (const n of Object.getOwnPropertyNames(Service.prototype)) anyS[n]("c", true);\n',
    },
    "prototype-access",
  ],
  "a reflective read": [
    { "src/other.ts": 'import { s } from "./use";\nReflect.get(s, "load").call(s, "c", true);\n' },
    "name-in-string",
  ],
  "a dependency type of the same shape": [
    {
      "src/other.ts": 'import { register } from "lib";\nimport { s } from "./use";\nregister(s);\n',
      "node_modules/lib/package.json": '{"name":"lib","types":"index.d.ts"}',
      "node_modules/lib/index.d.ts":
        "export declare function register(h: { load(id: string, cache?: boolean): string }): void;\n",
    },
    "external-structural",
  ],
};

for (const [name, [files, reason]] of Object.entries(HIDDEN_CALLS)) {
  test(`${name} blocks the method`, () => {
    const [result] = methodResults(
      { "src/svc.ts": STORE, "src/use.ts": STORE_USE, ...files },
      LOAD,
      { strict: true, module: "esnext", moduleResolution: "bundler" },
    );
    assert.deepEqual([result.outcome, result.reason], ["abstained", reason]);
  });
}

test("a method that the platform calls by name is not read", () => {
  const source =
    "export class Service {\n  toString(radix: number = 10): string { return String(radix); }\n}\n";
  const use =
    'import { Service } from "./svc";\nconst s = new Service();\ns.toString(16);\ns.toString(2);\n';
  const [result] = methodResults({ "src/svc.ts": source, "src/use.ts": use }, [
    { id: 0, path: "src/svc.ts", class: "Service", methods: ["toString"] },
  ]);
  assert.deepEqual([result.outcome, result.reason], ["abstained", "protocol-method"]);
});

test("a local class of the same name does not stand for the module class", () => {
  const local = [
    "function local() {",
    "  class Service {",
    '    other(): string { return "q"; }',
    "  }",
    "  return new Service().other();",
    "}",
    "void local;",
    "",
  ].join("\n");
  const [result] = methodResults(
    { "src/svc.ts": `${local}${STORE}`, "src/use.ts": STORE_USE },
    LOAD,
  );
  assert.equal(result.outcome, "complete");
  assert.equal(result.calls.length, 2);
});
