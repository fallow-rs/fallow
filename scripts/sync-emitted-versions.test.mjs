import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  auditEmittedVersions,
  findEmittedVersions,
  rewriteEmittedVersions,
  rewriteServerCard,
  serverCardVersions,
  workspaceVersion,
} from "./sync-emitted-versions.mjs";

const fallowOutputExample = `# Reference

\`\`\`json
{
  "kind": "agent-install",
  "schema_version": 1,
  "fallow_version": "3.16.0",
  "root": "/abs/path"
}
\`\`\`

\`\`\`json
{
  "kind": "health",
  "schema_version": 7,
  "version": "3.16.0",
  "elapsed_ms": 32
}
\`\`\`
`;

test("both emitted version keys are found in fallow output examples", () => {
  const found = findEmittedVersions(fallowOutputExample);

  assert.deepEqual(
    found.map((entry) => ({ key: entry.key, line: entry.line, version: entry.version })),
    [
      { key: "fallow_version", line: 7, version: "3.16.0" },
      { key: "version", line: 16, version: "3.16.0" },
    ],
  );
});

test("third-party versions never count as fallow output", () => {
  const source = `# Reference

A dependency example, not fallow output:

\`\`\`json
{
  "name": "demo-app",
  "version": "1.4.2",
  "dependencies": {
    "react": "19.0.0"
  }
}
\`\`\`

Dependency findings nested inside a fallow payload keep their own versions:

\`\`\`json
{
  "kind": "dead-code",
  "schema_version": 7,
  "version": "3.21.0",
  "unused_dependencies": [{ "name": "lodash", "version": "4.17.21" }],
  "tooling": { "name": "typescript", "version": "5.9.2" }
}
\`\`\`

Prose mentioning "version": "2.0.0" outside a code fence is not output either.

\`\`\`bash
fallow --version   # prints "version": "3.21.0"
\`\`\`
`;

  const found = findEmittedVersions(source);

  assert.deepEqual(
    found.map((entry) => ({ key: entry.key, version: entry.version })),
    [{ key: "version", version: "3.21.0" }],
  );
  assert.equal(rewriteEmittedVersions(source, "3.22.0"), source.replace('"3.21.0",', '"3.22.0",'));
});

test("a real dependency lockfile embedded as an example stays untouched", () => {
  const lockfile = readFileSync("tools/type-aware-sidecar/package-lock.json", "utf8");
  const source = ["```json", lockfile, "```"].join("\n");
  const thirdPartyVersions = lockfile.match(/"version": "[0-9]/gu) ?? [];

  assert.ok(thirdPartyVersions.length > 0, "the fixture must carry dependency versions");
  assert.deepEqual(findEmittedVersions(source), []);
});

test("an anchor that follows the version key still marks the object", () => {
  const source = [
    "```json",
    "{",
    '  "version": "3.20.0",',
    '  "schema_version": 7',
    "}",
    "```",
  ].join("\n");

  assert.equal(findEmittedVersions(source).length, 1);
});

test("a truncated example still reports its emitted version", () => {
  const source = [
    "```json",
    "{",
    '  "schema_version": 7,',
    '  "version": "3.20.0",',
    "  ...",
    "```",
  ].join("\n");

  assert.deepEqual(
    findEmittedVersions(source).map((entry) => entry.version),
    ["3.20.0"],
  );
});

test("rewriting updates every emitted key and leaves the document otherwise intact", () => {
  const updated = rewriteEmittedVersions(fallowOutputExample, "3.21.0");

  assert.equal(updated, fallowOutputExample.replaceAll("3.16.0", "3.21.0"));
  assert.deepEqual(
    findEmittedVersions(updated).map((entry) => entry.version),
    ["3.21.0", "3.21.0"],
  );
});

test("the server card carries the version in every package entry as well as the root", () => {
  const card = {
    name: "io.github.fallow-rs/fallow",
    version: "3.16.0",
    packages: [{ identifier: "fallow", version: "3.16.0" }],
  };

  assert.deepEqual(serverCardVersions(card), [
    { label: "version", version: "3.16.0" },
    { label: "packages[0].version", version: "3.16.0" },
  ]);
  assert.deepEqual(rewriteServerCard(card, "3.21.0"), {
    name: "io.github.fallow-rs/fallow",
    version: "3.21.0",
    packages: [{ identifier: "fallow", version: "3.21.0" }],
  });
  assert.deepEqual(Object.keys(rewriteServerCard(card, "3.21.0")), Object.keys(card));
});

test("the repository's emitted version strings match the workspace version", () => {
  const result = auditEmittedVersions({});

  assert.equal(result.version, workspaceVersion());
  assert.deepEqual(result.drift, []);
  assert.ok(
    result.sites.some((site) => site.path.startsWith("npm/fallow/skills/")),
    "the gate must cover the published skill contract",
  );
  assert.ok(
    result.sites.filter((site) => site.path === "server.json").length >= 2,
    "the gate must cover both server.json version fields",
  );
});

test("the gate catches a skill example that rotted several releases ago", () => {
  const reference = "npm/fallow/skills/fallow/references/cli-reference.md";
  const current = workspaceVersion();
  const rotted = readFileSync(reference, "utf8").replaceAll(`"${current}"`, '"3.16.0"');
  const found = findEmittedVersions(rotted);

  assert.ok(found.length > 0, "the reference must carry fallow output examples");
  assert.ok(
    found.every((entry) => entry.version === "3.16.0"),
    "every emitted string in the reference must be detected, not just the first key",
  );
  assert.deepEqual([...new Set(found.map((entry) => entry.key))].toSorted(), [
    "fallow_version",
    "version",
  ]);
});

test("server.json rewrites round-trip byte-identically", () => {
  const source = readFileSync("server.json", "utf8");
  const rewritten = `${JSON.stringify(rewriteServerCard(JSON.parse(source), workspaceVersion()), null, 2)}\n`;

  assert.equal(rewritten, source);
});

test("a comment quoting the anchor key does not anchor its object", () => {
  // JSON_LANGUAGES admits jsonc/json5, so prose inside a comment reaches the
  // scanner as ordinary text. Without comment skipping, this reads as a fallow
  // envelope and rewrites lodash's version.
  for (const comment of [
    '// "schema_version": 7 is what a fallow envelope looks like',
    '/* "schema_version": 7 */',
  ]) {
    const source = [
      "```jsonc",
      "{",
      `  ${comment}`,
      '  "name": "lodash",',
      '  "version": "4.17.21"',
      "}",
      "```",
    ].join("\n");

    assert.deepEqual(findEmittedVersions(source), [], `anchor inside ${comment} must not count`);
  }
});

test("a commented-out version inside a real envelope is left alone", () => {
  const source = [
    "```jsonc",
    "{",
    '  "schema_version": 7,',
    '  // "version": "4.17.21", <- the old pinned value, kept for reference',
    '  "version": "3.21.0"',
    "}",
    "```",
  ].join("\n");

  const found = findEmittedVersions(source);
  assert.equal(found.length, 1, "only the live value is a candidate");
  assert.equal(found[0].version, "3.21.0");

  const rewritten = rewriteEmittedVersions(source, "3.22.0");
  assert.ok(rewritten.includes('// "version": "4.17.21"'), "the commented pin survives verbatim");
  assert.ok(rewritten.includes('"version": "3.22.0"'), "the live value is rewritten");
});

test("the package script rejects a bad version before it reads a surface", () => {
  // A bad value stops the run in argument parsing, so the test writes no file.
  const result = spawnSync(
    process.execPath,
    ["scripts/sync-emitted-versions.mjs", "--check", "--version", "not-a-version"],
    { cwd: fileURLToPath(new URL("..", import.meta.url)), encoding: "utf8" },
  );
  assert.equal(result.status, 1);
  assert.equal(result.stdout, "");
  assert.equal(result.stderr, "--version expects a semantic version, got: not-a-version\n");
});
