import { describe, expect, test } from "vitest";
import { add } from "../src/math";

let addAttempts = 0;
let flakyAttempts = 0;

test("checks add on the first attempt only", () => {
  addAttempts += 1;
  if (addAttempts === 1) expect(add(2, 3)).toBe(5);
});

test("fails on the first attempt only", () => {
  flakyAttempts += 1;
  expect(flakyAttempts).toBeGreaterThan(1);
});

let testRetryAttempts = 0;
let suiteRetryAttempts = 0;

test("retries by test option", { retry: 2 }, () => {
  testRetryAttempts += 1;
  if (testRetryAttempts === 1) expect(add(2, 3)).toBe(5);
});

describe("retries by suite option", { retry: 2 }, () => {
  test("checks add on the first attempt only", () => {
    suiteRetryAttempts += 1;
    if (suiteRetryAttempts === 1) expect(add(2, 3)).toBe(5);
  });
});
