import { expect, it } from "vitest";

import { called } from "./called";
import type { typed } from "./typed";

type Typed = typeof typed;

it("calls only the value import", () => {
  const signature: Typed | undefined = undefined;
  expect(signature).toBeUndefined();
  expect(called()).toBe("called");
});
