import type { VizData, LayoutNode, GraphNode, ActiveView } from "./types";
import type { ColorTheme } from "./colors";
import { getTheme, detectDarkMode } from "./colors";

export interface AppState {
  data: VizData;
  canvas: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  activeView: ActiveView;
  // Treemap state
  currentPath: string[];
  hoveredIndex: number | null;
  layout: LayoutNode[];
  // Graph state
  graphNodes: GraphNode[];
  graphHoveredNode: number | null;
  graphFocusNode: number | null;
  graphFocusDepth: number;
  graphPan: { x: number; y: number };
  graphZoom: number;
  graphSimulating: boolean;
  // Shared
  darkMode: boolean;
  theme: ColorTheme;
  searchQuery: string;
  searchResults: Set<number>;
  dpr: number;
}

export const createState = (
  data: VizData,
  canvas: HTMLCanvasElement,
): AppState | null => {
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    document.body.textContent = "Error: Canvas 2D context unavailable.";
    return null;
  }
  const darkMode = detectDarkMode();

  return {
    data,
    canvas,
    ctx,
    activeView: "treemap",
    currentPath: [],
    hoveredIndex: null,
    layout: [],
    graphNodes: [],
    graphHoveredNode: null,
    graphFocusNode: null,
    graphFocusDepth: 2,
    graphPan: { x: 0, y: 0 },
    graphZoom: 1,
    graphSimulating: false,
    darkMode,
    theme: getTheme(darkMode),
    searchQuery: "",
    searchResults: new Set(),
    dpr: window.devicePixelRatio || 1,
  };
};

export const toggleDarkMode = (state: AppState): void => {
  state.darkMode = !state.darkMode;
  state.theme = getTheme(state.darkMode);
};
