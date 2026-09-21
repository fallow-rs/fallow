#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = fileURLToPath(new URL("..", import.meta.url));
const AUDIT_SOURCE_PATH = "crates/output/src/root_envelopes.rs";
const CHECK_SOURCE_PATH = "crates/output/src/check.rs";
const AUDIT_DOC_PATH = "cli/audit.mdx";
const COMPANION_DOCS_NAME = "fallow-docs";

/**
 * `git rev-parse --git-common-dir` for this checkout, or `null` when git cannot
 * answer. The value is relative (`.git`) in a primary checkout and absolute in a
 * linked worktree, and both spellings reduce to the main checkout.
 *
 * `GIT_DIR`, `GIT_WORK_TREE` and `GIT_INDEX_FILE` are stripped so an ambient
 * value from a hook or a wrapping tool cannot answer for another repository.
 */
const readGitCommonDir = (repoRoot) => {
  const env = { ...process.env };
  delete env.GIT_DIR;
  delete env.GIT_WORK_TREE;
  delete env.GIT_INDEX_FILE;
  const result = spawnSync("git", ["rev-parse", "--git-common-dir"], {
    cwd: repoRoot,
    encoding: "utf8",
    env,
  });
  if (result.error || result.status !== 0) {
    return null;
  }
  const value = result.stdout.trim();
  return value.length > 0 ? value : null;
};

/**
 * Where the companion documentation checkout is expected to live.
 *
 * `FALLOW_DOCS_DIR` wins when it carries a path. Otherwise the companion is a
 * sibling of the main checkout, which inside a linked worktree is not `repoRoot`:
 * the worktree lives under the main checkout, so resolving a sibling against it
 * points at a directory that never exists. An empty value carries no path, so it
 * falls back to the sibling; it still counts as naming the variable, which is
 * what keeps the check failing closed.
 */
export const companionDocsDir = ({ env = {}, gitCommonDir = null, repoRoot = REPO_ROOT } = {}) => {
  if (env.FALLOW_DOCS_DIR) {
    return resolve(env.FALLOW_DOCS_DIR);
  }
  const checkoutRoot = gitCommonDir === null ? repoRoot : dirname(resolve(repoRoot, gitCommonDir));
  return resolve(checkoutRoot, "..", COMPANION_DOCS_NAME);
};

export const parseRustSchemaVersion = (source, constantName) => {
  const pattern = new RegExp(`^pub const ${constantName}: u32 = ([0-9]+);$`, "gmu");
  const matches = [...source.matchAll(pattern)];
  if (matches.length !== 1) {
    throw new Error(`expected exactly one public u32 constant named ${constantName}`);
  }

  const version = Number.parseInt(matches[0][1], 10);
  if (!Number.isSafeInteger(version)) {
    throw new Error(`${constantName} is not a safe integer`);
  }
  return version;
};

export const expectedAuditSchemaVersions = ({ auditSource, checkSource }) => ({
  audit: parseRustSchemaVersion(auditSource, "AUDIT_SCHEMA_VERSION"),
  deadCode: parseRustSchemaVersion(checkSource, "CHECK_SCHEMA_VERSION"),
});

const auditJsonExample = (document) => {
  const pattern = /```json title="\$ fallow audit --format json"\r?\n([\s\S]*?)\r?\n```/gu;
  const matches = [...document.matchAll(pattern)];
  if (matches.length !== 1) {
    throw new Error("expected exactly one titled audit JSON example");
  }
  return matches[0][1].replaceAll("\r\n", "\n");
};

const schemaVersionLines = (example) =>
  [...example.matchAll(/^(\s*)"schema_version":\s*([0-9]+),?$/gmu)].map((match) => ({
    indentation: match[1].length,
    value: Number.parseInt(match[2], 10),
  }));

const directDeadCodeSchemaVersion = (example) => {
  const lines = example.split(/\r?\n/u);
  const start = lines.findIndex((line) => line === '  "dead_code": {');
  if (start === -1) {
    throw new Error("audit JSON example is missing the dead_code object");
  }

  const endOffset = lines.slice(start + 1).findIndex((line) => /^  \},?$/u.test(line));
  if (endOffset === -1) {
    throw new Error("audit JSON example has an unterminated dead_code object");
  }

  const body = lines.slice(start + 1, start + 1 + endOffset);
  const versions = body
    .map((line) => line.match(/^    "schema_version":\s*([0-9]+),?$/u))
    .filter(Boolean);
  if (versions.length !== 1) {
    throw new Error("dead_code must contain exactly one direct schema_version field");
  }
  return Number.parseInt(versions[0][1], 10);
};

export const checkAuditSchemaDoc = ({ document, expected }) => {
  const example = auditJsonExample(document);
  const occurrences = schemaVersionLines(example);
  if (occurrences.length !== 2) {
    throw new Error("audit JSON example must contain exactly two contextual schema versions");
  }

  const rootVersions = occurrences.filter(({ indentation }) => indentation === 2);
  if (rootVersions.length !== 1) {
    throw new Error("audit JSON example must contain exactly one root schema_version field");
  }

  const actualAudit = rootVersions[0].value;
  const actualDeadCode = directDeadCodeSchemaVersion(example);
  const drift = [];
  if (actualAudit !== expected.audit) {
    drift.push(`audit root is ${actualAudit}, expected ${expected.audit}`);
  }
  if (actualDeadCode !== expected.deadCode) {
    drift.push(`dead_code is ${actualDeadCode}, expected ${expected.deadCode}`);
  }
  if (drift.length > 0) {
    throw new Error(drift.join("; "));
  }

  return { audit: actualAudit, deadCode: actualDeadCode };
};

export const runAuditSchemaDocCheck = ({
  docsDir,
  env = process.env,
  gitCommonDir,
  repoRoot = REPO_ROOT,
} = {}) => {
  let expected;
  try {
    expected = expectedAuditSchemaVersions({
      auditSource: readFileSync(resolve(repoRoot, AUDIT_SOURCE_PATH), "utf8"),
      checkSource: readFileSync(resolve(repoRoot, CHECK_SOURCE_PATH), "utf8"),
    });
  } catch (error) {
    return {
      status: 2,
      message: `error: could not read canonical schema versions: ${error instanceof Error ? error.message : String(error)}`,
    };
  }

  // Presence, not content: an environment that sets the variable to an empty
  // string asked for the check, so it must not be able to buy a skip.
  const named = docsDir !== undefined || "FALLOW_DOCS_DIR" in env;
  const resolvedDocsDir =
    docsDir === undefined
      ? companionDocsDir({
          env,
          gitCommonDir: gitCommonDir === undefined ? readGitCommonDir(repoRoot) : gitCommonDir,
          repoRoot,
        })
      : resolve(docsDir);

  // Only a guessed checkout that is not there at all stands down: that is a
  // maintainer who has not cloned the companion, not documentation drift. A
  // named checkout, and any checkout that exists, still fails closed, so a
  // document that moved or was renamed is still reported.
  if (!named && !existsSync(resolvedDocsDir)) {
    return {
      status: 0,
      message: `skipped: no companion documentation checkout at ${resolvedDocsDir}; set FALLOW_DOCS_DIR to check parity`,
    };
  }

  const docPath = resolve(resolvedDocsDir, AUDIT_DOC_PATH);
  if (!existsSync(docPath)) {
    return { status: 1, message: `DRIFT: expected companion doc not found: ${docPath}` };
  }

  try {
    const actual = checkAuditSchemaDoc({
      document: readFileSync(docPath, "utf8"),
      expected,
    });
    return {
      status: 0,
      message: `audit schema docs match source contracts: audit=${actual.audit}, dead_code=${actual.deadCode}`,
    };
  } catch (error) {
    return {
      status: 1,
      message: `DRIFT: ${AUDIT_DOC_PATH}: ${error instanceof Error ? error.message : String(error)}`,
    };
  }
};

const main = () => {
  const result = runAuditSchemaDocCheck();
  const output = result.status === 0 ? console.log : console.error;
  output(result.message);
  process.exitCode = result.status;
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
