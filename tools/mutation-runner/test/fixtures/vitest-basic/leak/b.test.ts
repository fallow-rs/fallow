import { expect, test } from "vitest";
import { add } from "../src/math";

test("expects a fresh worker", () => {
  expect((globalThis as { fallowLeak?: boolean }).fallowLeak).toBeUndefined();
  expect(add(1, 1)).toBe(2);
});
