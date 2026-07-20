import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { checkRepositorySigningKeyParity } from "./signing-key-parity.mjs";

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

const markdownSection = (document, heading) => {
  const marker = `## ${heading}`;
  const start = document.indexOf(marker);
  assert.notEqual(start, -1, `missing ${heading} section`);

  const remaining = document.slice(start + marker.length);
  const nextHeading = remaining.indexOf("\n## ");
  return nextHeading === -1 ? remaining : remaining.slice(0, nextHeading);
};

const exportedNodeFunctions = (declarations) =>
  [...declarations.matchAll(/^export function ([A-Za-z_$][\w$]*)\(/gmu)].map((match) => match[1]);

const missingDocumentedNodeFunctions = (declarations, readme) => {
  const section = markdownSection(readme, "Editors and integrations");
  return exportedNodeFunctions(declarations).filter(
    (functionName) => !section.includes(`\`${functionName}\``),
  );
};

test("committed binary-signing public keys remain in parity", () => {
  assert.equal(checkRepositorySigningKeyParity().length, 32);
});

test("root Node API overview follows the published declarations", () => {
  const declarations = readFileSync("crates/napi/index.d.ts", "utf8");
  const readme = readFileSync("README.md", "utf8");
  const missing = missingDocumentedNodeFunctions(declarations, readme);

  assert.deepEqual(missing, [], `root Node API overview is missing: ${missing.join(", ")}`);
  assert.match(
    markdownSection(readme, "Editors and integrations"),
    /\[package API reference\]\(crates\/napi\/README\.md\)/u,
  );

  const firstFunction = exportedNodeFunctions(declarations)[0];
  const neuteredReadme = readme.replace(`\`${firstFunction}\``, "`removedFunction`");
  assert.deepEqual(missingDocumentedNodeFunctions(declarations, neuteredReadme), [firstFunction]);
});

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

test("root repository tooling declares its exact Node floor", () => {
  const rootPackage = readJson("package.json");
  const rootLock = readJson("package-lock.json");
  const contributing = readFileSync("CONTRIBUTING.md", "utf8");

  assert.equal(rootPackage.engines.node, ">=22.12.0");
  assert.equal(rootLock.packages[""].engines.node, ">=22.12.0");
  assert.match(contributing, /Repository tooling requires Node\.js 22\.12\.0 or later\./);
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

test("FALLOW_FORMAT docs include every GitHub-native format", () => {
  const formatSource = readFileSync("crates/cli/src/cli_format.rs", "utf8");
  const githubFormats = [...formatSource.matchAll(/#\[value\(name = "(github-[^"]+)"\)\]/g)].map(
    (match) => match[1],
  );
  assert.ok(githubFormats.length > 0, "Rust format catalog must include GitHub-native formats");

  const docs = readFileSync("docs/environment-variables.md", "utf8");
  const row = docs.match(/^\| `FALLOW_FORMAT` \| ([^|]+)\|/m);
  assert.ok(row, "environment variable docs must contain a FALLOW_FORMAT row");
  const documentedFormats = new Set([...row[1].matchAll(/`([^`]+)`/g)].map((match) => match[1]));

  for (const format of githubFormats) {
    assert.ok(documentedFormats.has(format), `FALLOW_FORMAT docs are missing ${format}`);
  }
});

test("narrator comment guard runs for commits, Claude, and CI", () => {
  const preCommit = readFileSync(".githooks/pre-commit", "utf8");
  const claudeSettings = readFileSync(".claude/settings.json", "utf8");
  const ci = readFileSync(".github/workflows/ci.yml", "utf8");

  assert.match(preCommit, /check-comment-quality\.mjs --staged/u);
  assert.match(claudeSettings, /check-comment-quality\.mjs.*--working-tree.*--claude-hook/u);
  assert.match(ci, /node scripts\/check-comment-quality\.mjs --all/u);
});
