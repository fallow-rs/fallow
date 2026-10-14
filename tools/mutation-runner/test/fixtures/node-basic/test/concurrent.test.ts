import assert from "node:assert/strict";
import { describe, test } from "node:test";

import { add, isPositive } from "../src/math.ts";

const wait = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

describe("overlap", { concurrency: true }, () => {
  test("adds later", async () => {
    await wait(60);
    assert.equal(add(2, 3), 5);
  });

  test("checks the sign of zero", async () => {
    const result = isPositive(0);
    await wait(150);
    assert.equal(result, false);
  });
});
