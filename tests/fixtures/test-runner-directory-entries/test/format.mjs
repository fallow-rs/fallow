import assert from "node:assert/strict";
import { test } from "node:test";
import { formatTotal } from "../lib/format.mjs";

test("formatTotal", () => {
  assert.equal(formatTotal([1, 2]), 3);
});
