import { expect, test } from "bun:test";
import { add } from "../src/math";

test("adds", () => {
  expect(add(1, 2)).toBe(3);
});
