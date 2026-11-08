import { appendFileSync } from "node:fs";
import { expect, test } from "vitest";
import { stays } from "../src/reuse";

test("stays", () => {
  const log = process.env.FALLOW_PID_LOG;
  if (log) appendFileSync(log, `${process.pid}\n`);
  expect(stays()).toBe(1);
});
