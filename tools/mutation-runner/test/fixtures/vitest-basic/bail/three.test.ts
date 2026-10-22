import { expect, inject, test } from "vitest";

// Each test fails when a mutant is active, so a fail-fast run stops after
// the first test.
for (const name of ["first", "second", "third"]) {
  test(name, () => {
    expect(inject("fallowActiveMutant" as never)).toBe(null);
  });
}
