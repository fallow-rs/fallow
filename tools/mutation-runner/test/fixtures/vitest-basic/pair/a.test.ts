import { appendFileSync } from "node:fs";
import { expect, test } from "vitest";
import { add } from "../src/math";

// Each file of this pair fails when the other file ran before it in the
// same worker, in either order.
test("runs before b", () => {
  const log = process.env.FALLOW_PID_LOG;
  if (log) appendFileSync(log, `${process.pid}\n`);
  const state = globalThis as { fallowPairA?: boolean; fallowPairB?: boolean };
  expect(state.fallowPairB).toBeUndefined();
  state.fallowPairA = true;
  expect(add(2, 3)).toBe(5);
});
