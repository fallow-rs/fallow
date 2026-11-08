import { appendFileSync } from "node:fs";
import { expect, test } from "vitest";
import { double } from "../src/reuse";

test("doubles", () => {
  const log = process.env.FALLOW_PID_LOG;
  if (log) appendFileSync(log, `${process.pid}\n`);
  expect(double(2)).toBe(4);
});
