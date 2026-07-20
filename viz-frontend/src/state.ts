import type { ActiveView, Lens, LayoutCell, RoadSelection, VizData } from "./types";
import type { DataIndex } from "./data";
import type { Theme } from "./theme";
import { getTheme, prefersReducedMotion } from "./theme";
import { buildIndex, reachSetMulti } from "./data";

export interface AppState {
  data: VizData;
  index: DataIndex;
  canvas: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  dpr: number;

  view: ActiveView;
  lens: Lens;

  /** Treemap drill path ("" = project root, else a directory path). */
  drillPath: string;
  /** Cached treemap layout for hit-testing (rebuilt per render). */
  layout: LayoutCell[];
  /** Index into `layout` currently hovered (cells, incl. directories). */
  hoveredCell: number | null;

  /** Selected file index (opens the detail panel). */
  selected: number | null;
  /** Selected aggregated road (graph overview drill-down). */
  selectedRoad: RoadSelection | null;
  /** Selected clone group index (duplication lens drill-down). */
  selectedClone: number | null;
  /** Hovered file index in graph view. */
  graphHovered: number | null;

  search: string;
  searchMatches: Set<number>;
  /** Union upstream reach of all current matches: the combined blast
   *  radius of a multi-file query (a PR's changed set). Empty for
   *  queries under two characters (too broad to be a meaningful set). */
  searchReach: Set<number>;

  dark: boolean;
  theme: Theme;
  reducedMotion: boolean;

  /** Help overlay visibility. */
  helpOpen: boolean;

  /** Set by main.ts; call after any state mutation. */
  requestRender: () => void;
}

export const createState = (data: VizData, canvas: HTMLCanvasElement): AppState | null => {
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  // Mode A dashboards are dark-first; the toggle switches to the light
  // (Mode B report) palette.
  const dark = true;
  return {
    data,
    index: buildIndex(data),
    canvas,
    ctx,
    dpr: window.devicePixelRatio || 1,
    view: "graph",
    lens: "overview",
    drillPath: "",
    layout: [],
    hoveredCell: null,
    selected: null,
    selectedRoad: null,
    selectedClone: null,
    graphHovered: null,
    search: "",
    searchMatches: new Set(),
    searchReach: new Set(),
    dark,
    theme: getTheme(dark),
    reducedMotion: prefersReducedMotion(),
    helpOpen: false,
    requestRender: () => {},
  };
};

export const setDarkMode = (state: AppState, dark: boolean): void => {
  state.dark = dark;
  state.theme = getTheme(dark);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
};

export const runSearch = (state: AppState, query: string): void => {
  state.search = query;
  state.searchMatches.clear();
  state.searchReach.clear();
  const normalizedQuery = query.trim().toLowerCase();
  if (normalizedQuery === "") return;
  for (let fileIndex = 0; fileIndex < state.data.files.length; fileIndex++) {
    if (state.data.files[fileIndex].path.toLowerCase().includes(normalizedQuery)) {
      state.searchMatches.add(fileIndex);
    }
  }
  // Combined blast radius of the matched set: everything that
  // transitively depends on any match. One multi-source traversal, so it
  // scales to broad subsystem queries (e.g. all "calendar" files) without
  // the per-match cap the old per-file loop needed. Skipped for one-char
  // queries, which match too much to be a meaningful set and would walk
  // the whole graph on every keystroke of a large monorepo.
  if (state.searchMatches.size > 0 && normalizedQuery.length >= 2) {
    for (const up of reachSetMulti(state.index.importersOf, state.searchMatches)) {
      state.searchReach.add(up);
    }
  }
};

// ── URL hash state (deep links for demos) ───────────────────────

const encodeHash = (state: AppState): string => {
  const parts: string[] = [`view=${state.view}`, `lens=${state.lens}`];
  if (state.drillPath) parts.push(`path=${encodeURIComponent(state.drillPath)}`);
  if (state.selected !== null) {
    parts.push(`file=${encodeURIComponent(state.data.files[state.selected].path)}`);
  }
  return parts.join("&");
};

export const applyHash = (state: AppState, hash: string): void => {
  const params = new URLSearchParams(hash.replace(/^#/, ""));
  const view = params.get("view");
  if (view === "map" || view === "graph") state.view = view;
  const lens = params.get("lens");
  if (
    lens === "overview" ||
    lens === "deadcode" ||
    lens === "dupes" ||
    lens === "boundaries" ||
    lens === "hotspots"
  ) {
    state.lens = lens;
  }
  const path = params.get("path");
  if (path && state.index.nodesByPath.has(path)) state.drillPath = path;
  const file = params.get("file");
  if (file) {
    const fileIndex = state.data.files.findIndex((fileEntry) => fileEntry.path === file);
    if (fileIndex !== -1) state.selected = fileIndex;
  }
};

export const syncHash = (state: AppState): void => {
  const next = `#${encodeHash(state)}`;
  if (window.location.hash !== next) {
    window.history.replaceState(null, "", next);
  }
};
