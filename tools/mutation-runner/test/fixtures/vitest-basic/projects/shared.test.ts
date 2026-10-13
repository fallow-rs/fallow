import { expect, inject, test } from "vitest";
import { add } from "../src/math";

// Two projects include this file. Only project "a" calls `add`, so only
// project "a" can detect a mutant in `add`.
test("checks per project", () => {
  if (inject("fixtureProject" as never) === "a") {
    expect(add(2, 3)).toBe(5);
  } else {
    expect(1).toBe(1);
  }
});
