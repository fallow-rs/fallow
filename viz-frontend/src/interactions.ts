import type { AppState } from "./state";
import { renderTreemap } from "./treemap";
import {
  renderGraph,
  graphHitTest,
  showGraphTooltip,
  handleGraphHover,
  handleGraphClick,
  graphGoBack,
  graphDragStart,
  graphDrag,
  graphDragEnd,
} from "./graph";
import { showTooltip, hideTooltip } from "./tooltip";
import { toggleDarkMode } from "./state";

// ── Treemap hit testing ─────────────────────────────────────────

const treemapHitTest = (state: AppState, x: number, y: number): number | null => {
  for (let i = state.layout.length - 1; i >= 0; i--) {
    const cell = state.layout[i];
    if (x >= cell.x && x <= cell.x + cell.w && y >= cell.y && y <= cell.y + cell.h) {
      return i;
    }
  }
  return null;
};

const getCanvasCoords = (
  state: AppState,
  e: MouseEvent,
): { x: number; y: number } => {
  const rect = state.canvas.getBoundingClientRect();
  return { x: e.clientX - rect.left, y: e.clientY - rect.top };
};

// ── Breadcrumb ──────────────────────────────────────────────────

const updateBreadcrumb = (
  state: AppState,
  breadcrumbEl: HTMLElement,
  render: (s: AppState) => void,
): void => {
  breadcrumbEl.replaceChildren();

  const rootLink = document.createElement("span");
  rootLink.className = "breadcrumb-segment breadcrumb-link";
  rootLink.textContent = state.data.root;
  rootLink.addEventListener("click", () => {
    state.currentPath = [];
    render(state);
    updateBreadcrumb(state, breadcrumbEl, render);
  });
  breadcrumbEl.appendChild(rootLink);

  for (let i = 0; i < state.currentPath.length; i++) {
    const sep = document.createElement("span");
    sep.className = "breadcrumb-sep";
    sep.textContent = " / ";
    breadcrumbEl.appendChild(sep);

    const segment = document.createElement("span");
    segment.className = "breadcrumb-segment";
    segment.textContent = state.currentPath[i];

    if (i < state.currentPath.length - 1) {
      segment.classList.add("breadcrumb-link");
      const depth = i + 1;
      segment.addEventListener("click", () => {
        state.currentPath = state.currentPath.slice(0, depth);
        render(state);
        updateBreadcrumb(state, breadcrumbEl, render);
      });
    }

    breadcrumbEl.appendChild(segment);
  }
};

// ── Event setup ─────────────────────────────────────────────────

export const setupInteractions = (
  state: AppState,
  breadcrumbEl: HTMLElement,
  searchInput: HTMLInputElement,
  summaryEl: HTMLElement,
  darkModeBtn: HTMLButtonElement,
  render: (s: AppState) => void,
): void => {
  const { canvas } = state;

  // ── Graph node dragging ─────────────────────────────────────
  // d3-zoom owns canvas pan; its filter (graph.ts) yields the press to us when
  // it lands on a node, so node drag and background pan never conflict.
  let draggingNode = false;
  canvas.addEventListener("mousedown", (e) => {
    if (state.activeView !== "graph" || e.button !== 0) return;
    const { x, y } = getCanvasCoords(state, e);
    const idx = graphHitTest(state, x, y);
    if (idx !== null) {
      graphDragStart(state, idx);
      draggingNode = true;
      canvas.style.cursor = "grabbing";
    }
  });
  window.addEventListener("mouseup", () => {
    if (draggingNode) {
      graphDragEnd(state);
      draggingNode = false;
      canvas.style.cursor = "grab";
    }
  });

  // ── Mouse move ──────────────────────────────────────────────
  canvas.addEventListener("mousemove", (e) => {
    if (draggingNode) {
      const { x, y } = getCanvasCoords(state, e);
      graphDrag(state, x, y);
      renderGraph(state);
      return;
    }

    const { x, y } = getCanvasCoords(state, e);

    if (state.activeView === "treemap") {
      const idx = treemapHitTest(state, x, y);
      if (idx !== state.hoveredIndex) {
        state.hoveredIndex = idx;
        canvas.style.cursor = idx !== null ? "pointer" : "default";
        renderTreemap(state);
      }
      if (idx !== null) {
        showTooltip(state.layout[idx], state.data, state.theme, e.clientX, e.clientY);
      } else {
        hideTooltip();
      }
    } else {
      const idx = graphHitTest(state, x, y);
      handleGraphHover(state, idx);
      canvas.style.cursor = idx !== null ? "pointer" : "grab";
      renderGraph(state);
      if (idx !== null) {
        showGraphTooltip(state, idx, e.clientX, e.clientY);
      } else {
        hideTooltip();
      }
    }
  });

  // ── Mouse leave ─────────────────────────────────────────────
  canvas.addEventListener("mouseleave", () => {
    if (state.activeView === "treemap") {
      state.hoveredIndex = null;
    } else {
      state.graphHoveredNode = null;
    }
    canvas.style.cursor = "default";
    hideTooltip();
    render(state);
  });

  // ── Click ───────────────────────────────────────────────────
  canvas.addEventListener("click", (e) => {
    const { x, y } = getCanvasCoords(state, e);

    if (state.activeView === "treemap") {
      const idx = treemapHitTest(state, x, y);
      if (idx === null) return;
      const cell = state.layout[idx];
      if (cell.node.fileIndex === null) {
        state.currentPath = cell.node.path.split("/");
        state.hoveredIndex = null;
        hideTooltip();
        renderTreemap(state);
        updateBreadcrumb(state, breadcrumbEl, render);
      }
    } else {
      const idx = graphHitTest(state, x, y);
      handleGraphClick(state, idx);
    }
  });

  // ── Double click (graph: clear focus) ───────────────────────
  canvas.addEventListener("dblclick", () => {
    if (state.activeView === "graph") {
      state.graphFocusNode = null;
      renderGraph(state);
    }
  });

  // Note: graph zoom/pan is handled by d3-zoom (attached in graph.ts initGraphNodes)

  // ── Keyboard ────────────────────────────────────────────────
  document.addEventListener("keydown", (e) => {
    if (document.activeElement === searchInput) return;

    if (e.key === "/" || (e.key === "f" && (e.metaKey || e.ctrlKey))) {
      e.preventDefault();
      searchInput.focus();
    } else if (e.key === "Escape") {
      if (state.activeView === "graph") {
        if (!graphGoBack(state)) {
          // Nothing to go back from, reset zoom/pan
          state.graphZoom = 1;
          state.graphPan = { x: 0, y: 0 };
          renderGraph(state);
        }
      } else if (state.activeView === "treemap" && state.currentPath.length > 0) {
        state.currentPath.pop();
        renderTreemap(state);
        updateBreadcrumb(state, breadcrumbEl, render);
      }
    } else if (e.key === "Backspace") {
      if (state.activeView === "treemap" && state.currentPath.length > 0) {
        state.currentPath.pop();
        renderTreemap(state);
        updateBreadcrumb(state, breadcrumbEl, render);
      }
    } else if (e.key === "1") {
      state.graphFocusDepth = 1;
      if (state.activeView === "graph") renderGraph(state);
    } else if (e.key === "2") {
      state.graphFocusDepth = 2;
      if (state.activeView === "graph") renderGraph(state);
    } else if (e.key === "3") {
      state.graphFocusDepth = 3;
      if (state.activeView === "graph") renderGraph(state);
    }
  });

  // ── Dark mode ───────────────────────────────────────────────
  darkModeBtn.addEventListener("click", () => {
    toggleDarkMode(state);
    document.body.style.backgroundColor = state.theme.bg;
    document.body.style.color = state.theme.text;
    summaryEl.style.backgroundColor = state.theme.summaryBg;
    breadcrumbEl.style.backgroundColor = state.theme.breadcrumbBg;
    breadcrumbEl.style.color = state.theme.breadcrumbText;
    darkModeBtn.textContent = state.darkMode ? "☀" : "☾";
    darkModeBtn.setAttribute("aria-pressed", String(state.darkMode));
    render(state);
  });

  // ── Resize ──────────────────────────────────────────────────
  let resizeTimer: ReturnType<typeof setTimeout>;
  window.addEventListener("resize", () => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => render(state), 100);
  });

  // Initial breadcrumb
  updateBreadcrumb(state, breadcrumbEl, render);
};
