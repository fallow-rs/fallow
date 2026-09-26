import { describe, expect, it } from "vitest";
import { attachHoverLayer, paintTreemapHover, treemapLayoutKey } from "./treemap";
import type { AppState } from "./state";
import { getTheme } from "./theme";
import type { LayoutCell, TreeNode } from "./types";

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
});

const DRAW_CALLS = new Set([
  "clearRect",
  "fillRect",
  "strokeRect",
  "fillText",
  "strokeText",
  "fill",
  "stroke",
  "drawImage",
]);

/** A 2D context stand-in that records every draw call. */
const countingContext = (): { ctx: CanvasRenderingContext2D; draws: string[] } => {
  const draws: string[] = [];
  const fields: Record<string, unknown> = {
    measureText: (text: string) => ({ width: text.length * 7 }),
  };
  const ctx = new Proxy(fields, {
    get: (target, key: string) => {
      if (key in target) return target[key];
      return (): void => {
        if (DRAW_CALLS.has(key)) draws.push(key);
      };
    },
    set: (target, key: string, value: unknown) => {
      target[key] = value;
      return true;
    },
  });
  return { ctx: ctx as unknown as CanvasRenderingContext2D, draws };
};

const fakeCanvas = (
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
): HTMLCanvasElement =>
  ({
    width,
    height,
    style: { width: `${width / 2}px`, height: `${height / 2}px` },
    getContext: () => ctx,
  }) as unknown as HTMLCanvasElement;

const node = (name: string, fileIndex: number | null, children: TreeNode[] = []): TreeNode => ({
  name,
  path: name,
  size: 10,
  children,
  fileIndex,
  parent: null,
});

const hoverState = (
  cells: LayoutCell[],
): { state: AppState; base: string[]; hover: string[]; hoverCanvas: HTMLCanvasElement } => {
  const base = countingContext();
  const hover = countingContext();
  const hoverCanvas = fakeCanvas(hover.ctx, 1, 1);
  const state = {
    view: "map",
    hoveredCell: 0,
    layout: cells,
    canvas: fakeCanvas(base.ctx, 800, 600),
    ctx: base.ctx,
    dpr: 2,
    theme: getTheme(true),
    search: "",
    searchMatches: new Set<number>(),
  } as unknown as AppState;
  attachHoverLayer(state, hoverCanvas);
  return { state, base: base.draws, hover: hover.draws, hoverCanvas };
};

describe("paintTreemapHover", () => {
  it("draws a file hover with a few calls on the hover layer only", () => {
    const cell: LayoutCell = { x: 10, y: 10, w: 80, h: 40, node: node("a.ts", 0), depth: 1 };
    const { state, base, hover, hoverCanvas } = hoverState([cell]);

    paintTreemapHover(state);

    expect(base).toEqual([]);
    expect(hover).toEqual(["clearRect", "fillRect", "strokeRect"]);
    expect(hoverCanvas.width).toBe(800);
    expect(hoverCanvas.height).toBe(600);
    expect(hoverCanvas.style.width).toBe("400px");
  });

  it("repaints the header band of a hovered folder", () => {
    const folder = node("src", null, [node("a.ts", 0)]);
    const cell: LayoutCell = { x: 0, y: 0, w: 300, h: 200, node: folder, depth: 0 };
    const { state, base, hover } = hoverState([cell]);

    paintTreemapHover(state);

    expect(base).toEqual([]);
    expect(hover).toEqual(["clearRect", "fillRect", "fillText", "fillText"]);
  });

  it("only clears the layer when nothing is hovered or the graph is shown", () => {
    const cell: LayoutCell = { x: 10, y: 10, w: 80, h: 40, node: node("a.ts", 0), depth: 1 };
    const { state, hover } = hoverState([cell]);

    state.hoveredCell = null;
    paintTreemapHover(state);
    state.hoveredCell = 0;
    state.view = "graph";
    paintTreemapHover(state);

    expect(hover).toEqual(["clearRect", "clearRect"]);
  });
});
