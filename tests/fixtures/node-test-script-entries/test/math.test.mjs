import { test } from "node:test";
import assert from "node:assert/strict";
import { clamp } from "../src/math.mjs";

test("clamp", () => {
  assert.equal(clamp(5, 0, 3), 3);
});
