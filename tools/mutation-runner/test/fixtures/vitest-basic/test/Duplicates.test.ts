import { expect, test } from "vitest";
import { add } from "../src/math";

test("repeats", () => {
  expect(add(1, 2)).toBe(3);
});

test("repeats", () => {
  expect(add(2, 2)).toBe(4);
});
