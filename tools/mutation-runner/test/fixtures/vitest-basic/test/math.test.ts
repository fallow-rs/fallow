import { describe, expect, test } from "vitest";
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
  expect((globalThis as { userSetupRan?: boolean }).userSetupRan).toBe(true);
});

test("noise", () => {
  console.log("console noise");
  process.stdout.write("raw stdout noise\n");
  expect(1).toBe(1);
});

test.skip("skipped", () => {
  expect(add(1, 2)).toBe(3);
});
