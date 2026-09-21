import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  checkAuditSchemaDoc,
  companionDocsDir,
  expectedAuditSchemaVersions,
  parseRustSchemaVersion,
  runAuditSchemaDocCheck,
} from "./check-audit-schema-doc-sync.mjs";

const auditDocument = ({ audit = 10, deadCode = 9, extra = "" } = {}) => `
## JSON output

\`\`\`json title="$ fallow audit --format json"
{
  "schema_version": ${audit},
  "version": "3.16.0",
  "command": "audit",
  "dead_code": {
    "schema_version": ${deadCode},
    "total_issues": 2
  }${extra}
}
\`\`\`

\`\`\`json title="$ fallow audit-cache prune --format json"
{
  "schema_version": 1,
  "command": "audit-cache prune"
}
\`\`\`
`;

test("schema constants come directly from their Rust declarations", () => {
  assert.equal(
    parseRustSchemaVersion("pub const AUDIT_SCHEMA_VERSION: u32 = 10;\n", "AUDIT_SCHEMA_VERSION"),
    10,
  );
  assert.throws(
    () => parseRustSchemaVersion("const AUDIT_SCHEMA_VERSION: u32 = 10;\n", "AUDIT_SCHEMA_VERSION"),
    /exactly one public u32 constant/u,
  );
  assert.throws(
    () =>
      parseRustSchemaVersion(
        "pub const AUDIT_SCHEMA_VERSION: u32 = 10;\npub const AUDIT_SCHEMA_VERSION: u32 = 11;\n",
        "AUDIT_SCHEMA_VERSION",
      ),
    /exactly one public u32 constant/u,
  );

  assert.deepEqual(
    expectedAuditSchemaVersions({
      auditSource: "pub const AUDIT_SCHEMA_VERSION: u32 = 10;\n",
      checkSource: "pub const CHECK_SCHEMA_VERSION: u32 = 9;\n",
    }),
    { audit: 10, deadCode: 9 },
  );
});

test("audit documentation checks the root and nested dead-code contracts only", () => {
  for (const document of [auditDocument(), auditDocument().replaceAll("\n", "\r\n")]) {
    assert.deepEqual(
      checkAuditSchemaDoc({
        document,
        expected: { audit: 10, deadCode: 9 },
      }),
      { audit: 10, deadCode: 9 },
    );
  }
});

test("audit documentation reports each contextual schema drift", () => {
  assert.throws(
    () =>
      checkAuditSchemaDoc({
        document: auditDocument({ audit: 3 }),
        expected: { audit: 10, deadCode: 9 },
      }),
    /audit root is 3, expected 10/u,
  );
  assert.throws(
    () =>
      checkAuditSchemaDoc({
        document: auditDocument({ deadCode: 3 }),
        expected: { audit: 10, deadCode: 9 },
      }),
    /dead_code is 3, expected 9/u,
  );
});

test("audit documentation fails closed on missing or extra schema contexts", () => {
  assert.throws(
    () =>
      checkAuditSchemaDoc({
        document: auditDocument().replace('    "schema_version": 9,\n', ""),
        expected: { audit: 10, deadCode: 9 },
      }),
    /exactly two contextual schema versions/u,
  );
  assert.throws(
    () =>
      checkAuditSchemaDoc({
        document: auditDocument({ extra: ',\n  "schema_version": 10' }),
        expected: { audit: 10, deadCode: 9 },
      }),
    /exactly two contextual schema versions/u,
  );
});

test("the companion checkout resolves as a sibling of the main checkout", () => {
  assert.equal(
    companionDocsDir({ gitCommonDir: ".git", repoRoot: "/checkouts/fallow" }),
    "/checkouts/fallow-docs",
  );
  // A linked worktree lives under the main checkout, so only the common git
  // directory names the checkout whose sibling the companion is.
  assert.equal(
    companionDocsDir({
      gitCommonDir: "/checkouts/fallow/.git",
      repoRoot: "/checkouts/fallow/.worktrees/topic",
    }),
    "/checkouts/fallow-docs",
  );
  assert.equal(
    companionDocsDir({ gitCommonDir: null, repoRoot: "/checkouts/fallow" }),
    "/checkouts/fallow-docs",
  );
  for (const gitCommonDir of [".git", "/checkouts/fallow/.git", null]) {
    assert.equal(
      companionDocsDir({
        env: { FALLOW_DOCS_DIR: "/elsewhere/docs" },
        gitCommonDir,
        repoRoot: "/checkouts/fallow/.worktrees/topic",
      }),
      "/elsewhere/docs",
    );
  }
});

test("a guessed companion checkout that is absent skips instead of reporting drift", () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-audit-schema-skip-"));
  const checkout = join(root, "checkout");
  mkdirSync(join(checkout, "crates", "output", "src"), { recursive: true });
  writeFileSync(
    join(checkout, "crates", "output", "src", "root_envelopes.rs"),
    "pub const AUDIT_SCHEMA_VERSION: u32 = 11;\n",
  );
  writeFileSync(
    join(checkout, "crates", "output", "src", "check.rs"),
    "pub const CHECK_SCHEMA_VERSION: u32 = 9;\n",
  );

  const skipped = runAuditSchemaDocCheck({ env: {}, gitCommonDir: ".git", repoRoot: checkout });
  assert.equal(skipped.status, 0);
  assert.match(skipped.message, /^skipped: no companion documentation checkout at /u);
  assert.match(skipped.message, /FALLOW_DOCS_DIR/u);

  // A named companion keeps failing closed, so continuous integration is unchanged.
  const named = join(root, "fallow-docs");
  assert.equal(runAuditSchemaDocCheck({ docsDir: named, env: {}, repoRoot: checkout }).status, 1);
  assert.equal(
    runAuditSchemaDocCheck({
      env: { FALLOW_DOCS_DIR: named },
      gitCommonDir: ".git",
      repoRoot: checkout,
    }).status,
    1,
  );

  // A variable that is set but empty names no path, so the sibling guess still
  // decides where to look, but it does ask for the check and cannot skip.
  const empty = runAuditSchemaDocCheck({
    env: { FALLOW_DOCS_DIR: "" },
    gitCommonDir: ".git",
    repoRoot: checkout,
  });
  assert.equal(empty.status, 1);
  assert.match(empty.message, /expected companion doc not found/u);

  // A companion checkout that exists but has lost the document is drift, even
  // when the path was guessed.
  mkdirSync(named, { recursive: true });
  const present = runAuditSchemaDocCheck({ env: {}, gitCommonDir: ".git", repoRoot: checkout });
  assert.equal(present.status, 1);
  assert.match(present.message, /expected companion doc not found/u);
});

test("audit documentation parity fails closed when the companion is absent", () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-audit-schema-parity-"));
  const result = runAuditSchemaDocCheck({ docsDir: join(root, "missing-docs") });

  assert.equal(result.status, 1);
  assert.match(result.message, /expected companion doc not found/u);
});

test("audit documentation parity fails closed when canonical sources are absent", () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-audit-schema-source-"));
  const result = runAuditSchemaDocCheck({ repoRoot: join(root, "missing-source") });

  assert.equal(result.status, 2);
  assert.match(result.message, /could not read canonical schema versions/u);
});
