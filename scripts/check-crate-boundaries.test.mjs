import assert from "node:assert/strict";
import test from "node:test";

import {
  findCrateBoundaryViolations,
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
      "foundation-must-not-depend-on-protocol",
      "output-must-not-start-analysis",
      "protocol-must-use-api-or-engine",
    ],
  );
});
