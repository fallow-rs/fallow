import { appendFileSync } from "node:fs";
import { expect, test } from "vitest";
import { add } from "../src/math";

test("adds", () => {
  const log = process.env.FALLOW_PID_LOG;
  if (log) appendFileSync(log, `${process.pid}\n`);
  expect(add(2, 3)).toBe(5);
});
