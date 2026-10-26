import { expect, test } from "vitest";
import { add } from "../src/math";

test("adds in each environment", () => {
  expect(add(2, 3)).toBe(5);
});
