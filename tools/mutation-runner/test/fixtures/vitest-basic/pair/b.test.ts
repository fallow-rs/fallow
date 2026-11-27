import { appendFileSync } from "node:fs";
import { expect, test } from "vitest";
import { add } from "../src/math";

test("runs before a", () => {
  const log = process.env.FALLOW_PID_LOG;
  if (log) appendFileSync(log, `${process.pid}\n`);
  const state = globalThis as { fallowPairA?: boolean; fallowPairB?: boolean };
  expect(state.fallowPairA).toBeUndefined();
  state.fallowPairB = true;
  expect(add(1, 1)).toBe(2);
});
