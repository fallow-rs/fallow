import assert from "node:assert/strict";
import test from "node:test";

import {
  findCrateBoundaryViolations,
  findProcessTreeOwnershipViolations,
  findSemanticOwnershipViolations,
  main,
  workspaceDependencyEdges,
} from "./check-crate-boundaries.mjs";

const metadataFor = (depsByPackage) => {
  const packages = Object.entries(depsByPackage).map(([name, deps]) => ({
    id: `path+file:///repo#${name}@0.0.0`,
    name,
    dependencies: deps.map((dep) => ({ name: dep })),
  }));

  return {
    packages,
    workspace_members: packages.map((pkg) => pkg.id),
  };
};

test("workspaceDependencyEdges keeps only workspace package edges", () => {
  const metadata = metadataFor({
    "fallow-api": ["fallow-engine", "serde"],
    "fallow-engine": ["fallow-types"],
    "fallow-types": [],
  });

  assert.deepEqual(workspaceDependencyEdges(metadata), [
    { from: "fallow-api", to: "fallow-engine" },
    { from: "fallow-engine", to: "fallow-types" },
  ]);
});

test("findCrateBoundaryViolations accepts current intended layering", () => {
  const metadata = metadataFor({
    "fallow-types": [],
    "fallow-process": [],
    "fallow-config": ["fallow-types"],
    "fallow-output": ["fallow-types"],
    "fallow-engine": ["fallow-core", "fallow-output"],
    "fallow-api": ["fallow-engine", "fallow-output"],
    "fallow-cli": ["fallow-api", "fallow-output"],
    "fallow-mcp": ["fallow-api"],
    "fallow-lsp": ["fallow-api"],
    "fallow-node": ["fallow-api"],
  });

  assert.deepEqual(findCrateBoundaryViolations(metadata), []);
});

test("findCrateBoundaryViolations rejects protocol and analysis back-edges", () => {
  const metadata = metadataFor({
    "fallow-process": ["fallow-types"],
    "fallow-types": ["fallow-cli"],
    "fallow-output": ["fallow-engine"],
    "fallow-lsp": ["fallow-core"],
    "fallow-cli": [],
    "fallow-core": [],
    "fallow-engine": [],
  });

  assert.deepEqual(
    findCrateBoundaryViolations(metadata).map((violation) => violation.rule),
    [
      "process-must-remain-independent",
      "foundation-must-not-depend-on-protocol",
      "output-must-not-start-analysis",
      "protocol-must-use-api-or-engine",
    ],
  );
});

test("semantic protocol ownership cannot move into protocol adapters", () => {
  const adapterFiles = [
    ["fallow-cli", "crates/cli/src/semantic_queries.rs"],
    ["fallow-lsp", "crates/lsp/src/semantic_queries.rs"],
    ["fallow-mcp", "crates/mcp/src/semantic_queries.rs"],
    ["fallow-node", "crates/napi/src/semantic_queries.rs"],
  ];

  for (const [crateName, path] of adapterFiles) {
    assert.deepEqual(
      findSemanticOwnershipViolations([
        {
          path,
          source: "struct SemanticRequest { protocol_version: u32 }",
        },
      ]),
      [
        {
          rule: "semantic-client-owned-by-api",
          from: crateName,
          to: "fallow-api",
          message: `${path} contains API-owned semantic client marker struct SemanticRequest`,
        },
      ],
    );
  }

  assert.deepEqual(
    findSemanticOwnershipViolations([
      {
        path: "crates/cli/src/check.rs",
        source: "fallow_api::refine_type_aware_results();",
      },
    ]),
    [],
  );
});

test("process-tree ownership cannot move into protocol adapters", () => {
  const path = "crates/mcp/src/tools/process_tree.rs";

  assert.deepEqual(
    findProcessTreeOwnershipViolations([
      {
        path,
        source: "command.process_group(0);",
      },
    ]),
    [
      {
        rule: "process-tree-owned-by-process-crate",
        from: "fallow-mcp",
        to: "fallow-process",
        message: `${path} contains process-tree owner marker process_group(0)`,
      },
    ],
  );

  assert.deepEqual(
    findProcessTreeOwnershipViolations([
      {
        path: "crates/mcp/src/tools/mod.rs",
        source: "fallow_process::configure_tokio_command(&mut command);",
      },
    ]),
    [],
  );
});

test("the live repository satisfies the complete crate boundary gate", () => {
  assert.equal(main([]), 0);
});
