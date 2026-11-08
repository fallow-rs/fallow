import { expect, test } from "vitest";
import { add } from "../src/math";

test("marks the module", () => {
  (globalThis as { fallowLeak?: boolean }).fallowLeak = true;
  expect(add(2, 3)).toBe(5);
});
