import { mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { withCodSpeed } from "@codspeed/tinybench-plugin";
import { Bench } from "tinybench";

import { WIRE_PROTOCOL_VERSION } from "../src/generated-protocol.mjs";
import { parseRequest } from "../src/protocol.mjs";
import { analyzeSemanticQueries, createSemanticSession } from "../src/semantic.mjs";

const sidecarRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));

const write = (root, relativePath, contents) => {
  const file = path.join(root, relativePath);
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, contents);
};

const createFixture = () => {
  const root = mkdtempSync(path.join(tmpdir(), "fallow-type-aware-bench-"));
  write(
    root,
    "tsconfig.json",
    JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
  );
  write(
    root,
    "src/repository.ts",
    ["export class UserRepository {", "  save(): void {}", "}", ""].join("\n"),
  );
  write(
    root,
    "src/consumer.ts",
    'import { UserRepository } from "./repository"; new UserRepository().save();\n',
  );
  return root;
};

const linkDependency = (root, dependency) => {
  const target = path.join(root, "node_modules", dependency);
  mkdirSync(path.dirname(target), { recursive: true });
  symlinkSync(
    path.join(sidecarRoot, "node_modules", dependency),
    target,
    process.platform === "win32" ? "junction" : "dir",
  );
};

const createTypeCouplingFixture = () => {
  const root = mkdtempSync(path.join(tmpdir(), "fallow-type-coupling-bench-"));
  write(
    root,
    "tsconfig.json",
    JSON.stringify({
      compilerOptions: {
        module: "NodeNext",
        moduleResolution: "NodeNext",
        skipLibCheck: true,
        strict: true,
        target: "ES2019",
      },
      include: ["src/**/*.ts"],
    }),
  );
  write(root, "package.json", JSON.stringify({ private: true, type: "module" }));
  write(
    root,
    "src/index.ts",
    [
      'import { z } from "zod";',
      "",
      "export const identitySchema = <T extends z.ZodTypeAny>(schema: T) => schema;",
      "",
    ].join("\n"),
  );
  write(
    root,
    "src/control.ts",
    [
      'import { z } from "zod";',
      "",
      "export const identitySchema = (schema: z.ZodTypeAny) => schema;",
      "",
    ].join("\n"),
  );
  linkDependency(root, "zod");
  return root;
};

const createRequest = (root) =>
  parseRequest({
    protocol_version: WIRE_PROTOCOL_VERSION,
    operation: "semantic-queries",
    root,
    projects: ["tsconfig.json"],
    evidence_limit: 40,
    queries: [
      {
        id: 0,
        operation: "symbol-use",
        symbol: {
          path: "src/repository.ts",
          namespace: "value",
          declaration_kind: "class_method",
          exported_name: "save",
          local_name: "save",
          owner: "UserRepository",
          line: 2,
          col: 2,
        },
        framework_contracts: [],
      },
    ],
  });

const createTypeCouplingRequest = (root, entryPoint, id) =>
  parseRequest({
    protocol_version: WIRE_PROTOCOL_VERSION,
    operation: "semantic-queries",
    root,
    projects: ["tsconfig.json"],
    evidence_limit: 40,
    queries: [
      {
        id,
        operation: "type-coupling",
        entry_points: [entryPoint],
        include_cycles: false,
      },
    ],
  });

const validTypeCouplingResult = (analysis) => {
  const result = analysis.results[0];
  return (
    result?.status === "complete" &&
    result.assertion === "no-coupling-found" &&
    result.data.edge_count === 0
  );
};

const root = createFixture();
const request = createRequest(root);
const session = createSemanticSession(root);
let revision = 1;
let latestResult = session.analyze(request, { revision });
const typeCouplingRoot = createTypeCouplingFixture();
const typeCouplingRequest = createTypeCouplingRequest(typeCouplingRoot, "src/index.ts", 1);
const typeCouplingControlRequest = createTypeCouplingRequest(typeCouplingRoot, "src/control.ts", 2);
const typeCouplingSession = createSemanticSession(typeCouplingRoot);
const typeCouplingControlSession = createSemanticSession(typeCouplingRoot);
let typeCouplingRevision = 1;
let typeCouplingControlRevision = 1;
let latestColdTypeCouplingResult = analyzeSemanticQueries(typeCouplingRequest);
let latestWarmTypeCouplingResult = typeCouplingSession.analyze(typeCouplingRequest, {
  revision: typeCouplingRevision,
});
let latestColdTypeCouplingControl = analyzeSemanticQueries(typeCouplingControlRequest);
let latestWarmTypeCouplingControl = typeCouplingControlSession.analyze(typeCouplingControlRequest, {
  revision: typeCouplingControlRevision,
});

const suite = withCodSpeed(new Bench());
suite
  .add("type-aware cold semantic analysis", () => {
    latestResult = analyzeSemanticQueries(request);
  })
  .add("type-aware warm semantic session", () => {
    revision += 1;
    latestResult = session.analyze(request, { revision });
  })
  .add("type-aware cold generic-constraint coupling", () => {
    latestColdTypeCouplingResult = analyzeSemanticQueries(typeCouplingRequest);
  })
  .add("type-aware warm generic-constraint coupling session", () => {
    typeCouplingRevision += 1;
    latestWarmTypeCouplingResult = typeCouplingSession.analyze(typeCouplingRequest, {
      revision: typeCouplingRevision,
    });
  })
  .add("type-aware cold non-generic coupling control", () => {
    latestColdTypeCouplingControl = analyzeSemanticQueries(typeCouplingControlRequest);
  })
  .add("type-aware warm non-generic coupling control session", () => {
    typeCouplingControlRevision += 1;
    latestWarmTypeCouplingControl = typeCouplingControlSession.analyze(typeCouplingControlRequest, {
      revision: typeCouplingControlRevision,
    });
  });

try {
  await suite.run();
  console.table(suite.table());
  if (latestResult.results[0]?.assertion !== "confirmed-used") {
    throw new Error("type-aware benchmark produced an unexpected semantic result");
  }
  if (
    !validTypeCouplingResult(latestColdTypeCouplingResult) ||
    !validTypeCouplingResult(latestWarmTypeCouplingResult) ||
    !validTypeCouplingResult(latestColdTypeCouplingControl) ||
    !validTypeCouplingResult(latestWarmTypeCouplingControl)
  ) {
    throw new Error("type-coupling benchmark produced an unexpected semantic result");
  }
} finally {
  session.close();
  typeCouplingSession.close();
  typeCouplingControlSession.close();
  rmSync(root, { recursive: true, force: true });
  rmSync(typeCouplingRoot, { recursive: true, force: true });
}
