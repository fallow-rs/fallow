import { appendFileSync } from "node:fs";
import { expect, test } from "vitest";
import { registered } from "../src/registry";
import { add } from "../src/math";

test("registers once", () => {
  const log = process.env.FALLOW_PID_LOG;
  if (log) appendFileSync(log, `${process.pid}\n`);
  expect(registered()).toBe(1);
  expect(add(1, 1)).toBe(2);
});
