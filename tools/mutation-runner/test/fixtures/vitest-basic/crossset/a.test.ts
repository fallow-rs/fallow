import { createRequire } from "node:module";
import { expect, test } from "vitest";
import { add } from "../src/math";

const state = createRequire(import.meta.url)("./state.cjs") as { runs: number };

test("counts a run", () => {
  state.runs += 1;
  expect(add(2, 3)).toBe(5);
});
