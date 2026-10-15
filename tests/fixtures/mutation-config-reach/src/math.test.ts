import { expect, test } from "vitest";
import { add } from "./math";

test("adds", () => {
  expect(add(1, 2)).toBe(3);
});
