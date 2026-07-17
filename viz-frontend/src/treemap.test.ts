import { describe, expect, it } from "vitest";
import { treemapLayoutKey } from "./treemap";

describe("treemapLayoutKey", () => {
  it("changes when drill, stage size, usable width, or dpr change", () => {
    const base = treemapLayoutKey("", 1600, 1000, 1600, 2);
    expect(treemapLayoutKey("src", 1600, 1000, 1600, 2)).not.toBe(base);
    expect(treemapLayoutKey("", 1400, 1000, 1400, 2)).not.toBe(base);
    // A panel opening shrinks only the usable width.
    expect(treemapLayoutKey("", 1600, 1000, 1220, 2)).not.toBe(base);
    expect(treemapLayoutKey("", 1600, 900, 1600, 2)).not.toBe(base);
    expect(treemapLayoutKey("", 1600, 1000, 1600, 1)).not.toBe(base);
  });

  it("is stable when only paint state (hover, selection) changes", () => {
    expect(treemapLayoutKey("src", 1600, 1000, 1220, 2)).toBe(
      treemapLayoutKey("src", 1600, 1000, 1220, 2),
    );
  });
});
