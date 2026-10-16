import { expect, it } from "vitest";

import { called } from "./called";
import { shaped } from "./shaped";
import type { ShapeName } from "./shapes";
import type { typed } from "./typed";

type Typed = typeof typed;
type Shaped = typeof shaped;

it("calls only the value import", () => {
  const signature: Typed | undefined = undefined;
  const shape: Shaped | undefined = undefined;
  const name: ShapeName = "square";
  expect(signature).toBeUndefined();
  expect(shape).toBeUndefined();
  expect(name).toBe("square");
  expect(called()).toBe("called");
});
