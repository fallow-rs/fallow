import { describe, expect, it } from "vitest";

import { add, clamp } from "./math";
import { save, size, target } from "./writer";
import { schemaVersion } from "./schema.generated";
import { render } from "./page";
import { sample } from "./fixtures/data";

describe("math", () => {
  it("adds", () => {
    expect(add(sample, 1 + 1)).toBe(7);
    expect(clamp(5, 0, 3)).toBe(3);
    expect(size("ab")).toBe(3);
    expect(schemaVersion()).toBe(2);
    expect(render()).toBe("cached");
    save("out.txt", "");
    expect(target("a", "b")).toBe("a/b.txt");
  });
});
