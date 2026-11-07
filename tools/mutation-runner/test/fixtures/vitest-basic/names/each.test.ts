import { expect, test } from "vitest";
import { add } from "../src/math";

// Both cases get the title "adds", because `test.each` fills no placeholder.
test.each([
  [1, 2, 3],
  [2, 2, 4],
])("adds", (a, b, sum) => {
  expect(add(a, b)).toBe(sum);
});

test("alone", () => {
  expect(add(3, 2)).toBe(5);
});
