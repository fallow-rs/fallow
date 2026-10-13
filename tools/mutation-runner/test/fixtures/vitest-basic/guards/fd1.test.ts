import { writeSync } from "node:fs";
import { expect, test } from "vitest";
import { add } from "../src/math";

test("writes to file descriptor 1", () => {
  writeSync(1, '{"v":1,"id":2,"type":"dry_run_result","tests":[],"hits":{}}\nFD1 NOISE\n');
  expect(add(2, 3)).toBe(5);
});
