import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import { runGenerationTransaction } from "./generation-transaction.mjs";

const write = (root, path, contents) => {
  const target = join(root, path);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, contents);
};

const read = (root, path) => readFileSync(join(root, path), "utf8");

const fixture = () => {
  const root = mkdtempSync(join(tmpdir(), "fallow-generation-transaction-test-"));
  const repoRoot = join(root, "repo");
  const stagingParent = join(root, "staging");
  mkdirSync(repoRoot);
  mkdirSync(stagingParent);
  return { root, repoRoot, stagingParent };
};

const cleanFixture = ({ root }) => rmSync(root, { force: true, recursive: true });

test("a late generation failure leaves destinations unchanged and removes staging", () => {
  const current = fixture();
  write(current.repoRoot, "one.txt", "old one");

  try {
    assert.throws(
      () =>
        runGenerationTransaction({
          repoRoot: current.repoRoot,
          stagingParent: current.stagingParent,
          surfacePaths: ["one.txt", "two.txt"],
          generate: (stagingRoot) => {
            write(stagingRoot, "one.txt", "new one");
            throw new Error("late phase failed");
          },
        }),
      /late phase failed/,
    );

    assert.equal(read(current.repoRoot, "one.txt"), "old one");
    assert.deepEqual(readdirSync(current.stagingParent), []);
  } finally {
    cleanFixture(current);
  }
});

test("staging must contain exactly the complete declared surface set", () => {
  const current = fixture();
  write(current.repoRoot, "one.txt", "old one");

  try {
    assert.throws(
      () =>
        runGenerationTransaction({
          repoRoot: current.repoRoot,
          stagingParent: current.stagingParent,
          surfacePaths: ["one.txt", "two.txt"],
          generate: (stagingRoot) => {
            write(stagingRoot, "one.txt", "new one");
            write(stagingRoot, "undeclared.txt", "extra");
          },
        }),
      /missing staged surfaces: two\.txt[\s\S]*undeclared staged surfaces: undeclared\.txt/,
    );

    assert.equal(read(current.repoRoot, "one.txt"), "old one");
    assert.deepEqual(readdirSync(current.stagingParent), []);
  } finally {
    cleanFixture(current);
  }
});

test("check mode reports drift without writing destinations", () => {
  const current = fixture();
  write(current.repoRoot, "one.txt", "old one");

  try {
    const result = runGenerationTransaction({
      repoRoot: current.repoRoot,
      stagingParent: current.stagingParent,
      surfacePaths: ["one.txt", "two.txt"],
      check: true,
      generate: (stagingRoot) => {
        write(stagingRoot, "one.txt", "new one");
        write(stagingRoot, "two.txt", "new two");
      },
    });

    assert.deepEqual(result.driftedPaths, ["one.txt", "two.txt"]);
    assert.equal(read(current.repoRoot, "one.txt"), "old one");
    assert.deepEqual(readdirSync(current.stagingParent), []);
  } finally {
    cleanFixture(current);
  }
});

test("successful generation promotes every changed surface and cleans staging", () => {
  const current = fixture();
  write(current.repoRoot, "one.txt", "old one");

  try {
    const result = runGenerationTransaction({
      repoRoot: current.repoRoot,
      stagingParent: current.stagingParent,
      surfacePaths: ["one.txt", "two.txt"],
      generate: (stagingRoot) => {
        write(stagingRoot, "one.txt", "new one");
        write(stagingRoot, "two.txt", "new two");
      },
    });

    assert.deepEqual(result.driftedPaths, ["one.txt", "two.txt"]);
    assert.equal(read(current.repoRoot, "one.txt"), "new one");
    assert.equal(read(current.repoRoot, "two.txt"), "new two");
    assert.deepEqual(readdirSync(current.stagingParent), []);
  } finally {
    cleanFixture(current);
  }
});

test("a promotion failure restores originals and removes newly created surfaces", () => {
  const current = fixture();
  write(current.repoRoot, "one.txt", "old one");

  try {
    assert.throws(
      () =>
        runGenerationTransaction({
          repoRoot: current.repoRoot,
          stagingParent: current.stagingParent,
          surfacePaths: ["one.txt", "two.txt"],
          generate: (stagingRoot) => {
            write(stagingRoot, "one.txt", "new one");
            write(stagingRoot, "two.txt", "new two");
          },
          beforePromote: (_path, index) => {
            if (index === 1) {
              throw new Error("promotion failed");
            }
          },
        }),
      /promotion failed/,
    );

    assert.equal(read(current.repoRoot, "one.txt"), "old one");
    assert.throws(() => read(current.repoRoot, "two.txt"), /ENOENT/);
    assert.deepEqual(readdirSync(current.stagingParent), []);
  } finally {
    cleanFixture(current);
  }
});
