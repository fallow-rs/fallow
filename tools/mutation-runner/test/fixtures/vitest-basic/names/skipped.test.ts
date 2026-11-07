import { expect, test } from "vitest";
import { add } from "../src/math";

// Twins that do not run give no kill evidence, so they are no duplicates.
test.todo("twin");
test.todo("twin");
test.skip("gone", () => {});
test.skip("gone", () => {});

test("solo", () => {
  expect(add(3, 2)).toBe(5);
});
