import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  checkAuditSchemaDoc,
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
