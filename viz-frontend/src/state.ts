import type {
  ActiveView,
  Lens,
  LayoutCell,
  RoadSelection,
  SecondaryAnalysis,
  VizData,
} from "./types";
import type { DataIndex } from "./data";
import type { Theme } from "./theme";
import { getTheme, prefersReducedMotion } from "./theme";
import { buildIndex, reachSetMulti } from "./data";
import { parseLens, parseSecondaryAnalysis } from "./lenses";

export type EntityMode = "files" | "components" | "packages";
export type DiffScope = "all" | "changed" | "introduced" | "affected";
export type FilterMode = "all" | "findings";

export interface AppState {
  data: VizData;
  index: DataIndex;
  canvas: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  dpr: number;

  view: ActiveView;
  lens: Lens;
  /** Secondary project analysis selected from More; the canvas stays on overview. */
  activeAnalysis: SecondaryAnalysis | null;
  /** Entity rendered by the map. Only files are available in this graph model. */
  entity: EntityMode;
  /** Diff scope. Non-default scopes become available when the payload contains a diff. */
  diffScope: DiffScope;
  /** File subset. Findings-only filtering requires graph support and is not yet available. */
  filterMode: FilterMode;

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
    activeAnalysis: null,
    entity: "files",
    diffScope: "all",
    filterMode: "all",
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

export const encodeHash = (state: AppState): string => {
  const parts: string[] = [
    `view=${state.view}`,
    `lens=${state.lens}`,
    `entity=${state.entity}`,
    `diff=${state.diffScope}`,
    `filter=${state.filterMode}`,
  ];
  if (state.activeAnalysis !== null) parts.push(`analysis=${state.activeAnalysis}`);
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
  const lens = parseLens(params.get("lens"));
  if (lens !== null) state.lens = lens;
  state.activeAnalysis = parseSecondaryAnalysis(params.get("analysis"));
  if (state.activeAnalysis !== null) state.lens = "overview";
  // The current graph model only has file nodes. Keeping unsupported entity
  // modes out of state avoids a deep link claiming that it rendered data it did not.
  if (params.get("entity") === "files") state.entity = "files";
  // Diff subsets require per-file diff metadata. Until that is in the payload,
  // only the honest all-files scope can be restored.
  if (params.get("diff") === "all") state.diffScope = "all";
  if (params.get("filter") === "all") state.filterMode = "all";
  const path = params.get("path");
  state.drillPath = path && state.index.nodesByPath.has(path) ? path : "";
  const file = params.get("file");
  state.selected = null;
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
