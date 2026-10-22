import assert from "node:assert/strict";
import fs, { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { sync } from "../src/backend.mjs";
import { parseRequest } from "../src/protocol.mjs";
import { analyzeSemanticQueries } from "../src/semantic.mjs";
import { ANALYSIS_OPERATION, WIRE_PROTOCOL_VERSION } from "../src/generated-protocol.mjs";

const COUNTED_CHECKER_METHODS = ["getAliasedSymbol", "getSymbolAtLocation"];

const write = (root, relativePath, contents) => {
  const fileName = path.join(root, relativePath);
  mkdirSync(path.dirname(fileName), { recursive: true });
  writeFileSync(fileName, contents);
};

const createFixture = (referenceCount) => {
  const root = mkdtempSync(path.join(tmpdir(), "fallow-type-aware-scan-cost-"));
  write(root, "tsconfig.json", JSON.stringify({ compilerOptions: { strict: true } }));
  write(
    root,
    "src/client.ts",
    ["export class Client {", "  used(): void {}", "  dead(): void {}", "}", ""].join("\n"),
  );
  write(root, "src/helpers.ts", "export const helper = (value: number): number => value;\n");
  write(
    root,
    "src/consumer.ts",
    [
      'import { Client } from "./client";',
      'import { helper as renamed } from "./helpers";',
      "new Client().used();",
      ...Array.from({ length: referenceCount }, (_, index) => `renamed(${index});`),
      "",
    ].join("\n"),
  );
  return root;
};

const memberQuery = (id, member, line) => ({
  id,
  operation: "symbol-use",
  symbol: {
    path: "src/client.ts",
    namespace: "value",
    declaration_kind: "class_method",
    exported_name: member,
    local_name: member,
    owner: "Client",
    line,
    col: 2,
  },
  framework_contracts: [],
});

// TypeScript 7.0 defines checker methods as plain methods. TypeScript 7.1 and
// tsrs define them as getters that return the bound method.
const countedDescriptor = (descriptor, count) =>
  descriptor.get
    ? {
        ...descriptor,
        get() {
          const method = descriptor.get.call(this);
          return (...args) => {
            count();
            return method(...args);
          };
        },
      }
    : {
        ...descriptor,
        value(...args) {
          count();
          return descriptor.value.apply(this, args);
        },
      };

const countCalls = (run) => {
  const counts = { realpath: 0 };
  const prototype = sync.Checker.prototype;
  const originals = COUNTED_CHECKER_METHODS.map((method) => [
    method,
    Object.getOwnPropertyDescriptor(prototype, method),
  ]);
  const originalRealpath = fs.realpathSync.native;
  for (const [method, descriptor] of originals) {
    counts[method] = 0;
    Object.defineProperty(
      prototype,
      method,
      countedDescriptor(descriptor, () => {
        counts[method] += 1;
      }),
    );
  }
  fs.realpathSync.native = (...args) => {
    counts.realpath += 1;
    return originalRealpath(...args);
  };
  try {
    return { result: run(), counts };
  } finally {
    for (const [method, descriptor] of originals)
      Object.defineProperty(prototype, method, descriptor);
    fs.realpathSync.native = originalRealpath;
  }
};

const scanFixture = (referenceCount) => {
  const root = createFixture(referenceCount);
  try {
    const request = parseRequest({
      protocol_version: WIRE_PROTOCOL_VERSION,
      operation: ANALYSIS_OPERATION,
      root,
      projects: ["tsconfig.json"],
      evidence_limit: 40,
      queries: [memberQuery(0, "used", 2), memberQuery(1, "dead", 3)],
    });
    return countCalls(() => analyzeSemanticQueries(request));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

test("symbol-use scan cost does not grow with unrelated identifier references", () => {
  const few = scanFixture(4);
  const many = scanFixture(64);

  for (const { result } of [few, many]) {
    assert.deepEqual(
      result.results.map(({ queryId, assertion }) => [queryId, assertion]),
      [
        [0, "confirmed-used"],
        [1, "confirmed-no-static-references"],
      ],
    );
  }
  assert.deepEqual(many.counts, few.counts);
});
