import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { parseRequest } from "../src/protocol.mjs";
import { ANALYSIS_OPERATION, WIRE_PROTOCOL_VERSION } from "../src/generated-protocol.mjs";

const sidecarRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const executable = process.env.FALLOW_TYPE_AWARE_BIN
  ? path.resolve(process.env.FALLOW_TYPE_AWARE_BIN)
  : path.join(sidecarRoot, "fallow-type-aware.mjs");

const write = (root, relativePath, contents) => {
  const fileName = path.join(root, relativePath);
  mkdirSync(path.dirname(fileName), { recursive: true });
  writeFileSync(fileName, contents);
};

const semanticRequest = (root, queries) => ({
  protocol_version: WIRE_PROTOCOL_VERSION,
  operation: ANALYSIS_OPERATION,
  root,
  projects: ["tsconfig.json"],
  evidence_limit: 40,
  queries,
});

const runSidecar = (body) =>
  JSON.parse(
    execFileSync(executable, {
      cwd: sidecarRoot,
      input: JSON.stringify(body),
      encoding: "utf8",
    }),
  );

// One shared model type and `moduleCount` public modules whose signatures use it.
const writeWideCouplingProject = (root, moduleCount) => {
  write(
    root,
    "tsconfig.json",
    JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
  );
  write(root, "src/model.ts", "export interface Model { value: string }\n");
  const reexports = [];
  for (let index = 0; index < moduleCount; index += 1) {
    write(
      root,
      `src/api${index}.ts`,
      `import type { Model } from "./model";\nexport interface Api${index} { model: Model }\n`,
    );
    reexports.push(`export type { Api${index} } from "./api${index}";`);
  }
  write(root, "src/index.ts", `${reexports.join("\n")}\n`);
};

const typeCouplingQuery = (extra = {}) => ({
  id: 40,
  operation: "type-coupling",
  entry_points: ["src/index.ts"],
  include_cycles: true,
  ...extra,
});

const withProject = (moduleCount, run) => {
  const root = mkdtempSync(path.join(tmpdir(), "fallow-type-aware-"));
  try {
    writeWideCouplingProject(root, moduleCount);
    run(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

test("type-coupling without a query limit keeps the request limit", () => {
  withProject(45, (root) => {
    const result = runSidecar(semanticRequest(root, [typeCouplingQuery()])).results[0];
    assert.ok(result.data.edge_count > 40);
    assert.equal(result.status, "partial");
    assert.equal(result.reason_code, "evidence-limit");
    assert.equal(result.data.edges.length, 40);
  });
});

test("type-coupling query evidence_limit returns a complete project-wide report", () => {
  withProject(45, (root) => {
    const result = runSidecar(
      semanticRequest(root, [typeCouplingQuery({ evidence_limit: 10_000 })]),
    ).results[0];
    assert.equal(result.status, "complete");
    assert.equal(result.truncated, false);
    assert.deepEqual(result.omissions, []);
    assert.equal(result.evidence.length, result.total_evidence_count);
    assert.equal(result.data.edges.length, result.data.edge_count);
    assert.equal(result.data.files.length, result.data.total_file_count);
    const model = result.data.files.find((file) => file.path === "src/model.ts");
    assert.ok(model.incoming_files.length > 40);
    assert.equal(model.incoming_files.length, model.total_incoming_file_count);
  });
});

test("type-coupling query evidence_limit still reports truncation", () => {
  withProject(3, (root) => {
    const result = runSidecar(semanticRequest(root, [typeCouplingQuery({ evidence_limit: 1 })]))
      .results[0];
    assert.equal(result.status, "partial");
    assert.equal(result.reason_code, "evidence-limit");
    assert.equal(result.evidence.length, 1);
    assert.equal(result.data.edges.length, 1);
  });
});

test("type-coupling query evidence_limit rejects values outside its bound", () => {
  for (const evidenceLimit of [0, 10_001, 1.5, "100"]) {
    assert.throws(
      () =>
        parseRequest(semanticRequest("/", [typeCouplingQuery({ evidence_limit: evidenceLimit })])),
      /evidence_limit/u,
    );
  }
});

test("only type-coupling queries accept a query evidence_limit", () => {
  assert.throws(
    () =>
      parseRequest(
        semanticRequest("/", [
          { id: 1, operation: "api-surface", entry_points: [], evidence_limit: 100 },
        ]),
      ),
    /evidence_limit/u,
  );
});
