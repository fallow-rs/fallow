import { describe, expect, it } from "vitest";
import { applyHash, encodeHash } from "./state";
import type { AppState } from "./state";

const state = (): AppState =>
  ({
    view: "graph",
    lens: "overview",
    activeAnalysis: null,
    entity: "files",
    diffScope: "all",
    filterMode: "all",
    drillPath: "",
    selected: null,
    data: { files: [{ path: "src/a.ts" }] },
    index: { nodesByPath: new Map([["src", {}]]) },
  }) as AppState;

describe("visualization URL state", () => {
  it("writes canonical lens, view, entity, diff, path and file state", () => {
    const appState = state();
    appState.view = "map";
    appState.lens = "security";
    appState.drillPath = "src";
    appState.selected = 0;

    expect(encodeHash(appState)).toBe(
      "view=map&lens=security&entity=files&diff=all&filter=all&path=src&file=src%2Fa.ts",
    );
  });

  it("restores legacy lens aliases as canonical IDs", () => {
    const appState = state();
    applyHash(appState, "#view=map&lens=boundaries&entity=files&diff=all");

    expect(appState.view).toBe("map");
    expect(appState.lens).toBe("architecture");
    expect(appState.entity).toBe("files");
    expect(appState.diffScope).toBe("all");
    expect(appState.filterMode).toBe("all");
  });

  it("restores secondary analysis state and keeps the canvas on overview", () => {
    const appState = state();
    applyHash(appState, "#view=graph&lens=security&analysis=dependencies");

    expect(appState.activeAnalysis).toBe("dependencies");
    expect(appState.lens).toBe("overview");
    expect(encodeHash(appState)).toContain("analysis=dependencies");
  });

  it("normalizes the legacy flags analysis alias", () => {
    const appState = state();
    applyHash(appState, "#analysis=flags");

    expect(appState.activeAnalysis).toBe("feature_flags");
  });

  it("does not activate unavailable entity or diff modes", () => {
    const appState = state();
    applyHash(appState, "#entity=components&diff=changed&filter=findings&lens=unknown");

    expect(appState.entity).toBe("files");
    expect(appState.diffScope).toBe("all");
    expect(appState.filterMode).toBe("all");
    expect(appState.lens).toBe("overview");
  });

  it("clears file and drill selection when a navigated hash omits them", () => {
    const appState = state();
    appState.drillPath = "src";
    appState.selected = 0;

    applyHash(appState, "#view=graph&lens=overview");

    expect(appState.drillPath).toBe("");
    expect(appState.selected).toBeNull();
  });
});
