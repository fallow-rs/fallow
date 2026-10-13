import { expect, it } from "vitest";

import { called } from "./called";
import { shaped } from "./shaped";
import type { typed } from "./typed";

type Typed = typeof typed;
type Shaped = typeof shaped;

it("calls only the value import", () => {
  const signature: Typed | undefined = undefined;
  const shape: Shaped | undefined = undefined;
  expect(signature).toBeUndefined();
  expect(shape).toBeUndefined();
  expect(called()).toBe("called");
});
