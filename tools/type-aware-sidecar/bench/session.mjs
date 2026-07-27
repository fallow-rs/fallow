import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { withCodSpeed } from "@codspeed/benchmark.js-plugin";
import Benchmark from "benchmark";

import { WIRE_PROTOCOL_VERSION } from "../src/generated-protocol.mjs";
import { parseRequest } from "../src/protocol.mjs";
import { analyzeSemanticQueries, createSemanticSession } from "../src/semantic.mjs";

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

const root = createFixture();
const request = createRequest(root);
const session = createSemanticSession(root);
let revision = 1;
let latestResult = session.analyze(request, { revision });

const suite = withCodSpeed(new Benchmark.Suite());
suite
  .add("type-aware cold semantic analysis", () => {
    latestResult = analyzeSemanticQueries(request);
  })
  .add("type-aware warm semantic session", () => {
    revision += 1;
    latestResult = session.analyze(request, { revision });
  })
  .on("cycle", (event) => {
    console.log(String(event.target));
  })
  .on("error", (event) => {
    console.error(event.target.error);
    process.exitCode = 1;
  })
  .on("complete", () => {
    session.close();
    rmSync(root, { recursive: true, force: true });
    if (latestResult.results[0]?.assertion !== "confirmed-used") {
      throw new Error("type-aware benchmark produced an unexpected semantic result");
    }
  })
  .run();
