import { appendFileSync } from "node:fs";
import { expect, test } from "vitest";

test("runs no source code", () => {
  const log = process.env.FALLOW_PLAIN_LOG;
  if (log) appendFileSync(log, `${process.pid}\n`);
  const pids = process.env.FALLOW_PID_LOG;
  if (pids) appendFileSync(pids, `${process.pid}\n`);
  expect(1 + 1).toBe(2);
});
