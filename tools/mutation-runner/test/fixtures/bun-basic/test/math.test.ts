import { describe, expect, test } from "bun:test";
import process from "node:process";

import { LIMIT, add, isPositive } from "../src/math";

describe("add", () => {
  test("adds numbers", () => {
    expect(add(2, 3)).toBe(5);
  });

  test("adds again", () => {
    expect(add(1, 1)).toBe(2);
  });
});

test("limit", () => {
  expect(LIMIT).toBe(10);
});

test("positive without an assertion", () => {
  isPositive(5);
});

test("noise", () => {
  console.log("console noise");
  process.stdout.write("raw stdout noise\n");
});

test.skip("skipped", () => {
  expect(add(1, 2)).toBe(3);
});

test.todo("todo later");
