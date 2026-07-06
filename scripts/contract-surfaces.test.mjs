import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { contractSurfacePaths, contractSurfaces } from "./contract-surfaces.mjs";
import { checkGithubActionsPathFilter } from "./check-contract-surfaces.mjs";

test("contract surface ids and generated paths are stable and unique", () => {
  const ids = contractSurfaces.map((surface) => surface.id);
  assert.deepEqual(ids, [...new Set(ids)]);

  for (const surface of contractSurfaces) {
    assert.match(surface.id, /^[a-z0-9-]+$/);
    assert.ok(surface.generatedPaths.length > 0, `${surface.id} has generated paths`);
    assert.ok(surface.checkCommand.length > 0, `${surface.id} has a check command`);
    assert.ok(surface.owner.length > 0, `${surface.id} has an owner`);

    for (const generatedPath of surface.generatedPaths) {
      assert.equal(generatedPath.startsWith("/"), false, generatedPath);
      assert.equal(generatedPath.includes("\\"), false, generatedPath);
    }
  }

  assert.deepEqual(contractSurfacePaths(), [...new Set(contractSurfacePaths())].toSorted());
});

test("current CI rust path filter covers generated contract surfaces", () => {
  const workflow = readFileSync(".github/workflows/ci.yml", "utf8");
  const result = checkGithubActionsPathFilter(workflow, contractSurfaces, {
    filterName: "rust",
  });

  assert.deepEqual(result.missing, []);
});

test("CI path filter check reports uncovered generated contract paths", () => {
  const workflow = `
jobs:
  changes:
    steps:
      - uses: dorny/paths-filter@v4
        with:
          filters: |
            rust:
              - 'schema.json'
`;

  const result = checkGithubActionsPathFilter(
    workflow,
    [
      {
        id: "fixture",
        owner: "fixture",
        generatedPaths: ["schema.json", "docs/output-schema.json"],
        checkCommand: "fixture",
        docs: [],
        publicStability: "stable",
        newIssueKind: true,
      },
    ],
    { filterName: "rust" },
  );

  assert.deepEqual(result.missing, [
    {
      path: "docs/output-schema.json",
      surfaceId: "fixture",
    },
  ]);
});
