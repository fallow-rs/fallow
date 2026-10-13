import { afterEach, beforeEach, inject } from "vitest";

import { installFallowMutation } from "./global.mjs";

const ACTIVE_MUTANT_KEY = "fallowActiveMutant";

const activeMutant = () => {
  const value = inject(ACTIVE_MUTANT_KEY);
  return typeof value === "string" ? value : null;
};

// The setup file runs before the test file imports any source module. The
// active mutant is set here at the top level, so a mutant in module scope
// (a static mutant) is active when the module evaluates.
const mutation = installFallowMutation();
mutation.startFile();
mutation.setActive(activeMutant());

beforeEach(() => {
  mutation.setActive(activeMutant());
  mutation.startTest();
});

afterEach((context) => {
  const { hits, staticHits } = mutation.finishTest();
  context.task.meta.fallowHits = hits;
  context.task.meta.fallowStaticHits = staticHits;
});
