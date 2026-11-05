import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { ANALYSIS_OPERATION, WIRE_PROTOCOL_VERSION } from "../src/generated-protocol.mjs";

const sidecarRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const executable = process.env.FALLOW_TYPE_AWARE_BIN
  ? path.resolve(process.env.FALLOW_TYPE_AWARE_BIN)
  : path.join(sidecarRoot, "fallow-type-aware.mjs");

const REQUEST_EVIDENCE_LIMIT = 40;

const write = (root, relativePath, contents) => {
  const fileName = path.join(root, relativePath);
  mkdirSync(path.dirname(fileName), { recursive: true });
  writeFileSync(fileName, contents);
};

const runSidecar = (body) =>
  JSON.parse(
    execFileSync(executable, {
      cwd: sidecarRoot,
      input: JSON.stringify(body),
      encoding: "utf8",
    }),
  );

const runMemberQuery = (root) =>
  runSidecar({
    protocol_version: WIRE_PROTOCOL_VERSION,
    operation: ANALYSIS_OPERATION,
    root,
    projects: ["tsconfig.json"],
    evidence_limit: REQUEST_EVIDENCE_LIMIT,
    queries: [
      {
        id: 0,
        operation: "symbol-use",
        symbol: {
          path: "src/worker.ts",
          namespace: "value",
          declaration_kind: "class_method",
          exported_name: "run",
          local_name: "run",
          owner: "Worker",
          line: 2,
          col: 2,
        },
        framework_contracts: [],
      },
    ],
  }).results[0];

// `Worker.run` and a main module that calls it `callCount` times.
const withWorkerProject = (callCount, extraMain, run) => {
  const root = mkdtempSync(path.join(tmpdir(), "fallow-type-aware-"));
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/worker.ts", "export class Worker {\n  run() { return 42; }\n}\n");
    const calls = Array.from(
      { length: callCount },
      () => "console.log(identity(new Worker()).run());",
    );
    write(
      root,
      "src/main.ts",
      [
        'import { Worker } from "./worker";',
        "function identity<T>(value: T): T { return value; }",
        ...calls,
        ...extraMain,
        "",
      ].join("\n"),
    );
    run(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

for (const callCount of [1, 2, REQUEST_EVIDENCE_LIMIT + 1]) {
  test(`symbol-use with ${callCount} exact call(s) is a complete confirmed use`, () => {
    withWorkerProject(callCount, [], (root) => {
      const result = runMemberQuery(root);
      assert.equal(result.assertion, "confirmed-used");
      assert.equal(result.status, "complete");
      assert.equal(result.reason_code, null);
      assert.deepEqual(result.omissions, []);
      assert.deepEqual(result.actions, []);
      assert.equal(result.total_evidence_count, callCount);
      assert.equal(result.data.total_reference_count, callCount);
      assert.equal(result.evidence.length, 1);
      assert.equal(result.truncated, callCount > 1);
    });
  });
}

const callerModule = (callCount) =>
  [
    'import { Worker } from "./worker";',
    "function identity<T>(value: T): T { return value; }",
    ...Array.from(
      { length: callCount },
      () => "export const call = (): number => identity(new Worker()).run();",
    ).map((line, index) => line.replace("call", `call${index}`)),
    "",
  ].join("\n");

// The client accepts a use only from a reachable file. The result must therefore
// show each file that references the member, not only the first file.
test("symbol-use returns one evidence item for each referencing file", () => {
  withWorkerProject(1, [], (root) => {
    write(root, "src/a-orphan.ts", callerModule(3));
    const result = runMemberQuery(root);
    assert.equal(result.assertion, "confirmed-used");
    assert.equal(result.status, "complete");
    assert.equal(result.total_evidence_count, 4);
    assert.equal(result.truncated, true);
    assert.deepEqual(
      result.evidence.map((item) => item.path),
      ["src/a-orphan.ts", "src/main.ts"],
    );
  });
});

test("symbol-use evidence for more files than the limit stays a complete use", () => {
  withWorkerProject(0, [], (root) => {
    const fileCount = REQUEST_EVIDENCE_LIMIT + 1;
    for (let index = 0; index < fileCount; index += 1) {
      write(root, `src/caller-${String(index).padStart(2, "0")}.ts`, callerModule(1));
    }
    const result = runMemberQuery(root);
    assert.equal(result.assertion, "confirmed-used");
    assert.equal(result.status, "complete");
    assert.deepEqual(result.omissions, []);
    assert.equal(result.total_evidence_count, fileCount);
    assert.equal(result.evidence.length, REQUEST_EVIDENCE_LIMIT);
    assert.equal(new Set(result.evidence.map((item) => item.path)).size, REQUEST_EVIDENCE_LIMIT);
    assert.equal(result.truncated, true);
  });
});

test("symbol-use keeps an unrelated dynamic member access as a partial use", () => {
  const dynamicAccess = [
    "const unrelated: Record<string, number> = {};",
    "const dynamicKey: string = String(Date.now());",
    "console.log(unrelated[dynamicKey]);",
  ];
  withWorkerProject(1, dynamicAccess, (root) => {
    const result = runMemberQuery(root);
    assert.equal(result.assertion, "confirmed-used");
    assert.equal(result.status, "partial");
    assert.deepEqual(result.omissions, [{ reason_code: "dynamic-member-access", count: 1 }]);
    assert.equal(result.truncated, false);
  });
});
