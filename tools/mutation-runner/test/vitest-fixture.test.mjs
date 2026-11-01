import assert from "node:assert/strict";
import { test } from "node:test";

import { createFixtureProject } from "./support/vitest-fixture.mjs";

test("a fixture for a Vitest package that is not installed names the install command", () => {
  assert.throws(
    () => createFixtureProject({ packageName: "vitest-not-installed" }),
    (error) => {
      assert.match(error.message, /vitest-not-installed is not installed at/u);
      assert.match(error.message, /npm ci --prefix tools\/mutation-runner/u);
      return true;
    },
  );
});
