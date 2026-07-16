import { applyHash, createState, runSearch, setDarkMode, syncHash } from "./state";
import type { AppState } from "./state";
import type { Lens } from "./types";
import {
  captureLensColors,
  drillInto,
  drillTo,
  drillUp,
  renderTreemap,
  startLensFade,
  treemapHitTest,
} from "./treemap";
import {
  centerOnFile,
  getClusterMode,
  graphDrag,
  graphDragEnd,
  graphDragStart,
  graphHitTest,
  initGraphNodes,
  isDragging,
  renderGraph,
  setClusterMode,
} from "./graph";
import { buildChrome, statuslineOf, updateChrome } from "./chrome";
import type { ChromeRefs } from "./chrome";
import { createPanel, renderPanel } from "./panel";
import { hideTooltip, showDirTooltip, showFileTooltip } from "./tooltip";
import { dirname } from "./data";

const renderView = (state: AppState): void => {
  if (state.view === "map") {
    renderTreemap(state);
  } else {
    initGraphNodes(state);
  }
};

const init = (): void => {
  const data = window.__FALLOW_DATA__;
  if (!data) {
    document.body.textContent = "Error: no fallow visualization data found.";
    return;
  }

  document.documentElement.dataset.theme = "dark";

  const app = document.createElement("div");
  app.id = "app";
  document.body.appendChild(app);

  // Stage (canvas + overlays)
  const stage = document.createElement("main");
  stage.id = "stage";
  const canvas = document.createElement("canvas");
  canvas.id = "canvas";
  canvas.tabIndex = 0;

  const state = createState(data, canvas);
  if (!state) {
    document.body.textContent = "Error: canvas 2D context unavailable.";
    return;
  }
  setDarkMode(state, state.dark);
  applyHash(state, window.location.hash);

  canvas.setAttribute("role", "img");
  canvas.setAttribute(
    "aria-label",
    `Interactive map of ${data.summary.total_files} files in ${data.root}. ` +
      "Use the map and graph buttons to switch views, the lens buttons to change " +
      "what the colors mean, and the search box to find files.",
  );

  // Chrome
  let refs: ChromeRefs | null = null;
  const rerenderChrome = (): void => {
    if (refs) updateChrome(state, refs);
  };

  const setLens = (lens: Lens): void => {
    if (state.lens === lens) return;
    const prev = captureLensColors(state);
    state.lens = lens;
    startLensFade(state, prev);
    requestRender();
  };

  const setView = (view: "map" | "graph"): void => {
    if (state.view === view) return;
    state.view = view;
    state.hoveredCell = null;
    state.graphHovered = null;
    hideTooltip();
    requestRender();
  };

  refs = buildChrome(state, app, {
    onView: setView,
    onLens: setLens,
    onSearch: (query) => {
      runSearch(state, query);
      requestRender();
    },
    onTheme: () => {
      setDarkMode(state, !state.dark);
      requestRender();
    },
    onCrumb: () => {},
    onCluster: (mode) => {
      if (refs) {
        for (const [m, b] of refs.clusterButtons) {
          b.setAttribute("aria-pressed", String(m === mode));
        }
      }
      setClusterMode(state, mode);
    },
  });
  refs.crumbHandler = (path: string) => {
    drillTo(state, path);
    requestRender();
  };

  stage.appendChild(canvas);
  stage.appendChild(refs.emptyState);
  const panel = createPanel();
  stage.appendChild(panel);
  app.appendChild(stage);
  app.appendChild(statuslineOf(refs));

  // ── Navigation shared by panel + views ────────────────────────
  const selectFile = (fileIndex: number | null, reveal = false): void => {
    state.selected = fileIndex;
    if (fileIndex !== null && reveal) {
      if (state.view === "map") {
        const dir = dirname(state.data.files[fileIndex].path);
        // Drill to the nearest ancestor directory that exists as a node and
        // contains the file, so the selection is visible.
        if (!state.data.files[fileIndex].path.startsWith(state.drillPath === "" ? "" : `${state.drillPath}/`)) {
          let target = dir;
          while (target !== "" && !state.index.nodesByPath.has(target)) {
            target = dirname(target);
          }
          state.drillPath = target;
        }
      } else {
        centerOnFile(state, fileIndex);
      }
    }
    requestRender();
  };

  // ── Render loop (rAF-coalesced) ───────────────────────────────
  let renderQueued = false;
  const requestRender = (): void => {
    if (renderQueued) return;
    renderQueued = true;
    requestAnimationFrame(() => {
      renderQueued = false;
      rerenderChrome();
      renderPanel(state, panel, (idx) => selectFile(idx, true), () => selectFile(null));
      renderView(state);
      syncHash(state);
    });
  };
  state.requestRender = requestRender;

  // ── Canvas interactions ───────────────────────────────────────
  let mouseDownAt: { x: number; y: number } | null = null;
  let dragMoved = false;

  const canvasPoint = (e: MouseEvent): { x: number; y: number } => {
    const rect = canvas.getBoundingClientRect();
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  };

  canvas.addEventListener("mousemove", (e) => {
    const { x, y } = canvasPoint(e);
    if (state.view === "map") {
      const hit = treemapHitTest(state, x, y);
      if (hit !== state.hoveredCell) {
        state.hoveredCell = hit;
        requestRender();
      }
      if (hit !== null) {
        const cell = state.layout[hit];
        canvas.style.cursor = "pointer";
        if (cell.node.fileIndex !== null) {
          showFileTooltip(state, cell.node.fileIndex, e.clientX, e.clientY);
        } else {
          showDirTooltip(
            cell.node.name,
            countLeaves(cell.node),
            cell.node.size,
            e.clientX,
            e.clientY,
          );
        }
      } else {
        canvas.style.cursor = "default";
        hideTooltip();
      }
    } else {
      if (isDragging(state)) {
        dragMoved = true;
        graphDrag(state, x, y);
        renderGraph(state);
        return;
      }
      const hit = graphHitTest(state, x, y);
      if (hit !== state.graphHovered) {
        state.graphHovered = hit;
        renderGraph(state);
      }
      canvas.style.cursor = hit !== null ? "pointer" : "grab";
      if (hit !== null) {
        showFileTooltip(state, hit, e.clientX, e.clientY);
      } else {
        hideTooltip();
      }
    }
  });

  canvas.addEventListener("mouseleave", () => {
    hideTooltip();
    if (state.view === "map" && state.hoveredCell !== null) {
      state.hoveredCell = null;
      requestRender();
    }
    if (state.view === "graph" && state.graphHovered !== null) {
      state.graphHovered = null;
      renderGraph(state);
    }
  });

  canvas.addEventListener("mousedown", (e) => {
    if (e.button !== 0) return;
    const { x, y } = canvasPoint(e);
    mouseDownAt = { x, y };
    dragMoved = false;
    if (state.view === "graph") {
      const hit = graphHitTest(state, x, y);
      if (hit !== null) graphDragStart(state, hit);
    }
  });

  window.addEventListener("mouseup", (e) => {
    if (state.view === "graph" && isDragging(state)) {
      graphDragEnd(state);
    }
    if (!mouseDownAt) return;
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const moved =
      dragMoved || Math.abs(x - mouseDownAt.x) > 4 || Math.abs(y - mouseDownAt.y) > 4;
    mouseDownAt = null;
    if (moved || e.target !== canvas) return;

    // Treat as a click.
    if (state.view === "map") {
      const hit = treemapHitTest(state, x, y);
      if (hit === null) return;
      const cell = state.layout[hit];
      if (cell.node.fileIndex !== null) {
        selectFile(cell.node.fileIndex);
      } else {
        hideTooltip();
        drillInto(state, cell);
        requestRender();
      }
    } else {
      const hit = graphHitTest(state, x, y);
      selectFile(hit);
    }
  });

  // ── Keyboard ──────────────────────────────────────────────────
  const lensOrder: Lens[] = ["deadcode", "dupes", "boundaries", "hotspots"];
  window.addEventListener("keydown", (e) => {
    const target = e.target as HTMLElement | null;
    const inInput = target?.tagName === "INPUT" || target?.tagName === "TEXTAREA";

    if (e.key === "Escape") {
      if (inInput && refs) {
        refs.search.value = "";
        runSearch(state, "");
        refs.search.blur();
        requestRender();
        return;
      }
      if (state.selected !== null) {
        selectFile(null);
      } else if (state.search !== "" && refs) {
        refs.search.value = "";
        runSearch(state, "");
        requestRender();
      } else if (state.view === "map" && drillUp(state)) {
        requestRender();
      }
      return;
    }
    if (inInput) return;

    if (e.key === "/") {
      e.preventDefault();
      refs?.search.focus();
    } else if (e.key >= "1" && e.key <= "4") {
      setLens(lensOrder[Number(e.key) - 1]);
    } else if (e.key === "m") {
      setView("map");
    } else if (e.key === "g") {
      setView("graph");
    }
  });

  window.addEventListener("resize", () => requestRender());
  window.addEventListener("hashchange", () => {
    applyHash(state, window.location.hash);
    requestRender();
  });

  // Initial paint.
  requestRender();

  // Keep the cluster segment in sync with the actual mode at boot.
  for (const [m, b] of refs.clusterButtons) {
    b.setAttribute("aria-pressed", String(m === getClusterMode(state)));
  }
};

const countLeaves = (node: { children: Array<{ children: unknown[]; fileIndex: number | null }>; fileIndex: number | null }): number => {
  if (node.fileIndex !== null) return 1;
  let n = 0;
  for (const child of node.children) {
    n += countLeaves(child as typeof node);
  }
  return n;
};

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", init);
} else {
  init();
}
