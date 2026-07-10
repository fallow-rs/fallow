import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

const documentedFields = (guide, heading) => {
  const marker = `### ${heading}`;
  const start = guide.indexOf(marker);
  assert.notEqual(start, -1, `missing ${heading} section`);

  const remaining = guide.slice(start + marker.length);
  const nextHeading = remaining.indexOf("\n### ");
  const section = nextHeading === -1 ? remaining : remaining.slice(0, nextHeading);

  return [...section.matchAll(/^\|\s*`([^`]+)`\s*\|/gm)].map((match) => match[1]);
};

test("published Node packages and Action smoke tests use Node 22", () => {
  const packagePaths = ["npm/fallow/package.json", "crates/napi/package.json"];

  for (const path of packagePaths) {
    assert.equal(readJson(path).engines.node, ">=22", path);
  }

  const napiLock = readJson("crates/napi/package-lock.json");
  assert.equal(napiLock.packages[""].engines.node, ">=22", "NAPI lock root metadata");

  const actionWorkflow = readFileSync(".github/workflows/test-action.yml", "utf8");
  const versions = [...actionWorkflow.matchAll(/^\s+node-version:\s*['"]?(\d+)['"]?\s*$/gm)].map(
    (match) => match[1],
  );
  assert.ok(versions.length > 0, "Action workflow must select a Node runtime");
  assert.deepEqual([...new Set(versions)], ["22"]);
});

test("CONTRIBUTING uses the root contract generation commands", () => {
  const contributing = readFileSync("CONTRIBUTING.md", "utf8");

  assert.match(contributing, /^npm run generate:contracts$/m);
  assert.match(contributing, /^npm run generate:contracts:check$/m);
});

test("plugin authoring guide documents every top-level schema field", () => {
  const guide = readFileSync("docs/plugin-authoring.md", "utf8");
  const schema = readJson("plugin-schema.json");
  const documented = [
    ...documentedFields(guide, "Required"),
    ...documentedFields(guide, "Optional"),
  ].toSorted();
  const schemaFields = Object.keys(schema.properties).toSorted();

  assert.deepEqual(documented, schemaFields);
});
