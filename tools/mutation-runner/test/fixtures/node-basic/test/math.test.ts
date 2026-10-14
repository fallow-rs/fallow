import assert from "node:assert/strict";
import { describe, test } from "node:test";

import { LIMIT, add, isPositive } from "../src/math.ts";

describe("add", () => {
  test("adds numbers", () => {
    assert.equal(add(2, 3), 5);
  });

  test("adds again", () => {
    assert.equal(add(1, 1), 2);
  });
});

test("limit", () => {
  assert.equal(LIMIT, 10);
});

test("positive without an assertion", () => {
  isPositive(5);
});

test("noise", () => {
  console.log("console noise");
  process.stdout.write("raw stdout noise\n");
});

test.skip("skipped", () => {
  assert.equal(add(1, 2), 3);
});

test.todo("todo later");
