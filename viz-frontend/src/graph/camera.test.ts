import { describe, expect, it, vi } from "vitest";
import type { AppState } from "../state";
import { applyZoom } from "./build";
import { renderGraph } from "./render";
import { getGVS } from "./shared";

vi.mock("./render", () => ({ renderGraph: vi.fn() }));

describe("graph camera", () => {
  it("does not paint the graph over the map view", () => {
    const state = { view: "map", selected: null } as unknown as AppState;
    const gvs = getGVS(state);

    applyZoom(state, gvs, { x: 10, y: 20, k: 2 }, false);

    expect(gvs.transform).toEqual({ x: 10, y: 20, k: 2 });
    expect(renderGraph).not.toHaveBeenCalled();

    state.view = "graph";
    applyZoom(state, gvs, { x: 0, y: 0, k: 1 }, true);

    expect(renderGraph).toHaveBeenCalledTimes(1);
    expect(gvs.userMoved).toBe(true);
  });
});
