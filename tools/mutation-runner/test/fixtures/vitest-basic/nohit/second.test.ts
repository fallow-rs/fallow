import { appendFileSync } from "node:fs";
import { expect, test } from "vitest";
import { unused } from "../src/unused";

test("adds", () => {
  const log = process.env.FALLOW_PID_LOG;
  if (log) appendFileSync(log, `${process.pid}\n`);
  expect(unused()).toBe(1);
});
