import { expect, test } from "vitest";
import { isAllowed } from "./auth";

test("a level above two is allowed", () => {
  expect(isAllowed(3)).toBe(true);
});
