import { createRequire } from "node:module";
import { expect, test } from "vitest";
import { add } from "../src/math";

const state = createRequire(import.meta.url)("./state.cjs") as { runs: number };

test("expects no run of a before it", () => {
  expect(state.runs).toBe(0);
  expect(add(1, 1)).toBe(2);
});
