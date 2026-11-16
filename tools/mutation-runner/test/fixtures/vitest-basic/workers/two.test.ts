import { appendFileSync } from "node:fs";
import { expect, test } from "vitest";
import { add } from "../src/math";

test("records the worker of file two", () => {
  appendFileSync(process.env.FALLOW_POOL_LOG as string, `${process.env.VITEST_POOL_ID}\n`);
  expect(add(2, 3)).toBe(5);
});
