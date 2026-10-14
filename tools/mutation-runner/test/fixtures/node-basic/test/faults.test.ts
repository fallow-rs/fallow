import assert from "node:assert/strict";
import { test } from "node:test";

import { pause, settle, shout } from "../src/faults.ts";

test("settles", () => {
  assert.equal(settle(1), 1);
});

test("pauses", async () => {
  await pause();
});

test("shouts", () => {
  assert.equal(shout("a"), "A");
});
