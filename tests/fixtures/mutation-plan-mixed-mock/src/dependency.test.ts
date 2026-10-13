import { describe, expect, it } from "vitest";

import { renderDependency } from "./dependency";

describe("renderDependency", () => {
  it("renders the real text", () => {
    expect(renderDependency()).toBe("real dependency");
  });
});
