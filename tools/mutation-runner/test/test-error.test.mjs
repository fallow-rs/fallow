import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, describe, test } from "node:test";
import { pathToFileURL } from "node:url";

import { MAX_TEST_ERROR_LENGTH, errorDirectories, testErrorText } from "../src/adapters/common.mjs";

describe("the error text of a failed test", () => {
  const base = mkdtempSync(join(tmpdir(), "fallow-test-error-"));
  const root = join(base, "project");
  const runDir = join(base, "run");
  const directories = errorDirectories(root, runDir);

  after(() => rmSync(base, { recursive: true, force: true }));

  test("keeps the name, the message and the project frames, relative to the project", () => {
    const error = {
      name: "AssertionError",
      message: "expected true to be false",
      stack: [
        "AssertionError: expected true to be false",
        `    at ${pathToFileURL(join(root, "src/flag.test.ts")).href}:17:5`,
        `    at flag (${join(runDir, "src/flag.ts")}:1:20)`,
        `    at run (${join(root, "node_modules/vitest/dist/index.js")}:9:1)`,
        "    at process.processTicksAndRejections (node:internal/process/task_queues:105:5)",
        "    at new Promise (<anonymous>)",
      ].join("\n"),
    };
    assert.equal(
      testErrorText(error, directories),
      [
        "AssertionError: expected true to be false",
        "    at src/flag.test.ts:17:5",
        "    at flag (src/flag.ts:1:20)",
      ].join("\n"),
    );
  });

  test("removes color codes", () => {
    const error = { name: "Error", message: "\u001b[31mred\u001b[39m text" };
    assert.equal(testErrorText(error, directories), "Error: red text");
  });

  test("does not repeat a name that the message already has", () => {
    const error = { name: "TimeoutError", message: "TimeoutError: test timed out" };
    assert.equal(testErrorText(error, directories), "TimeoutError: test timed out");
  });

  test("is bounded in size", () => {
    const error = { name: "Error", message: "x".repeat(MAX_TEST_ERROR_LENGTH * 4) };
    const text = testErrorText(error, directories);
    assert.ok(text.length <= MAX_TEST_ERROR_LENGTH + 3, `${text.length}`);
    assert.ok(text.endsWith("..."));
  });

  test("is null with no error", () => {
    assert.equal(testErrorText(undefined, directories), null);
    assert.equal(testErrorText(null, directories), null);
  });
});
