import assert from "node:assert/strict";
import { before, describe, test } from "node:test";

import { shout } from "../src/faults.ts";

describe("with a setup hook", () => {
  let loud = "";
  before(() => {
    loud = shout("a");
  });

  test("reads the hook value", () => {
    assert.equal(loud, "A");
  });
});
