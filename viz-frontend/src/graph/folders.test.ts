import { expect, it } from "vitest";
import { graphInitSteps } from "./build";
import type { AppState } from "../state";
import { getTheme } from "../theme";
import { renderEgoStage } from "./ego";
import { graphHandleClick } from "./interact";
import { getGVS } from "./shared";

it("collapses an expanded importer folder when its heading is clicked", () => {
  const labels = new Map<string, [number, number]>();
  const ctx = new Proxy(
    {
      measureText: (label: string) => ({ width: label.length * 6 }),
      fillText: (label: string, x: number, y: number) => labels.set(label, [x, y]),
    },
    { get: (target, key) => Reflect.get(target, key) ?? (() => {}) },
  );
  const files = ["root.ts", "a/1.ts", "a/2.ts", "a/3.ts", "b/4.ts", "b/5.ts"].map((path) => ({
    path,
  }));
  const state = {
    ctx,
    data: { files },
    selected: 0,
    reducedMotion: true,
    theme: getTheme(false),
    lens: "unused",
    index: {
      importersOf: [[1, 2, 3, 4, 5]],
      importsOf: [[]],
      violationEdges: new Set(),
      cycleEdges: new Set(),
      dynamicEdges: new Set(),
    },
  } as unknown as AppState;
  const gvs = getGVS(state);
  const draw = (): void => {
    labels.clear();
    renderEgoStage(state, gvs, 1600, 260);
  };
  const click = (label: string): void => {
    const point = labels.get(label);
    if (!point) throw new Error(`Missing graph label: ${label}`);
    graphHandleClick(state, ...point);
  };

  draw();
  click("a/ (3)");
  draw();
  expect(labels.has("1.ts")).toBe(true);
  click("A/");
  draw();
  expect(labels.has("a/ (3)")).toBe(true);
  expect(labels.has("1.ts")).toBe(false);
});

it("splits large sibling modules below a shared directory", () => {
  const files = ["accounts", "billing"].flatMap((name) =>
    Array.from({ length: 101 }, (_, i) => ({ path: `src/features/${name}/${i}.ts` })),
  );
  const state = { data: { files, edges: [] } } as unknown as AppState;
  const steps = graphInitSteps(state);
  for (const _step of steps) {
    if (getGVS(state).clusters.length > 0) break;
  }
  expect(getGVS(state).clusters.map((group) => [group.key, group.indices.length])).toEqual([
    ["src/features/accounts", 101],
    ["src/features/billing", 101],
  ]);
});

it("keeps direct files and a small subfolder together below a shared directory", () => {
  const files = Array.from({ length: 150_000 }, (_, index) => ({ path: `src/shared/${index}.ts` }));
  files.push({ path: "src/shared/sub/entry.ts" });
  const state = { data: { files, edges: [] } } as unknown as AppState;
  for (const _step of graphInitSteps(state)) {
    if (getGVS(state).clusters.length > 0) break;
  }
  expect(getGVS(state).clusters.map((group) => [group.key, group.indices.length])).toEqual([
    ["src/shared", files.length],
  ]);
});
