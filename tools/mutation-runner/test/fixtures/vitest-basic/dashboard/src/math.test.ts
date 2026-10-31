import { expect, it } from "vitest";

import { add } from "../../src/math";

it("adds values", () => expect(add(2, 3)).toBe(5));
