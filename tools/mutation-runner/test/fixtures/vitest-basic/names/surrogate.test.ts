import { expect, test } from "vitest";
import { add } from "../src/math";

test("lone \uD800", () => {
  expect(add(1, 2)).toBe(3);
});

test("lone \uD801", () => {
  expect(add(2, 2)).toBe(4);
});

test("pair 😀", () => {
  expect(add(3, 2)).toBe(5);
});
