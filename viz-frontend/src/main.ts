import { applyHash, createState, runSearch, setDarkMode, syncHash } from "./state";
import type { AppState } from "./state";
import type { Lens, TreeNode } from "./types";
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
  clearGraphFocus,
  clearRoadHover,
  dismissIntro,
  getClusterMode,
  graphFocusSearch,
  graphHandleClick,
  graphHoverTarget,
  graphPathTrace,
  initGraphNodes,
  nodeScreenPos,
  refitOnResize,
  usableStageWidth,
  minimapHit,
  minimapPan,
  renderGraph,
  resetEgoTrail,
  resetGraphView,
  roadFacts,
  setClusterMode,
  startGraphLensFade,
} from "./graph";
import { buildHelpOverlay } from "./overlays";
import { buildChrome, statuslineOf, updateChrome } from "./chrome";
import type { ChromeRefs } from "./chrome";
import { createPanel, panelRenderKey, renderPanel } from "./panel";
import { hideTooltip, showDirTooltip, showFileTooltip, showRoadTooltip } from "./tooltip";
import { installHintTips } from "./hint";
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
      "what the colors mean, and the search box to find files. The ranked list on " +
      "the right opens any file and shows the worst findings for the current lens.",
  );

  // A brief opacity dip when the canvas content changes wholesale (view
  // swap, leaving ego mode), so the swap reads as a transition, not a cut.
  canvas.addEventListener("animationend", () => canvas.classList.remove("swapping"));
  const flashCanvas = (): void => {
    if (state.reducedMotion) return;
    canvas.classList.remove("swapping");
    void canvas.offsetWidth; // reflow so re-adding restarts the animation
    canvas.classList.add("swapping");
  };

  // Chrome
  let refs: ChromeRefs | null = null;
  const rerenderChrome = (): void => {
    if (refs) updateChrome(state, refs);
  };

  const setLens = (lens: Lens): void => {
    if (state.lens === lens) return;
    const prev = captureLensColors(state);
    state.lens = lens;
    state.selectedClone = null;
    startLensFade(state, prev);
    // Crossfade the graph nodes too, so 1-5 animates in both views.
    if (state.view === "graph") startGraphLensFade(state, prev);
    // The ranked panel opens or closes with the lens; keep the graph
    // fitted to the space that remains while the camera is untouched.
    if (state.view === "graph") refitOnResize(state);
    requestRender();
  };

  const setView = (view: "map" | "graph"): void => {
    if (state.view === view) return;
    state.view = view;
    state.hoveredCell = null;
    state.graphHovered = null;
    hideTooltip();
    // The two views draw completely different structures; a short dip
    // makes the swap read as the same data redrawn, not a glitch.
    flashCanvas();
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
        for (const [clusterMode, button] of refs.clusterButtons) {
          button.setAttribute("aria-pressed", String(clusterMode === mode));
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
  // Keyboard-focus ring for the full-width canvas, drawn as an overlay over
  // just the usable area so it never runs under the always-open panel (a CSS
  // outline on the canvas would frame the whole element, panel included).
  const canvasFocusRing = document.createElement("div");
  canvasFocusRing.className = "canvas-focus-ring";
  canvasFocusRing.setAttribute("aria-hidden", "true");
  stage.appendChild(canvasFocusRing);
  // The arrange toggle floats over the map (top-right, graph view only);
  // chrome builds it but leaves mounting to the stage here.
  stage.appendChild(refs.clusterGroup);
  const panel = createPanel();
  stage.appendChild(panel);
  app.appendChild(stage);
  app.appendChild(statuslineOf(refs));

  // Floating [data-tip] hints: one body-level element, delegated globally so
  // it survives panel rebuilds and escapes the panel's overflow clip.
  installHintTips();

  // ── Help overlay ──────────────────────────────────────────────
  // Where focus returns when the modal help dialog closes.
  let helpReturnFocus: HTMLElement | null = null;
  const restoreHelpFocus = (): void => {
    helpReturnFocus?.focus();
    helpReturnFocus = null;
  };
  const helpOverlay = buildHelpOverlay({
    onHelpClose: () => {
      state.helpOpen = false;
      helpOverlay.classList.remove("open");
      restoreHelpFocus();
    },
  });
  document.body.appendChild(helpOverlay);

  const toggleHelp = (): void => {
    state.helpOpen = !state.helpOpen;
    helpOverlay.classList.toggle("open", state.helpOpen);
    if (state.helpOpen) {
      // Move focus into the dialog and remember where it came from.
      helpReturnFocus =
        document.activeElement instanceof HTMLElement ? document.activeElement : null;
      helpOverlay.querySelector<HTMLButtonElement>("button.close")?.focus();
    } else {
      restoreHelpFocus();
    }
  };
  refs.helpHandler = toggleHelp;

  // ── Navigation shared by panel + views ────────────────────────
  const selectFile = (fileIndex: number | null, reveal = false): void => {
    const hadSelection = state.selected !== null;
    state.selected = fileIndex;
    if (fileIndex === null) resetEgoTrail(state);
    // Leaving the graph ego stage swaps back to the overview; soften that
    // return the same way as a view swap so it does not read as a reset.
    if (fileIndex === null && hadSelection && state.view === "graph") flashCanvas();
    if (fileIndex !== null && reveal) {
      if (state.view === "map") {
        const dir = dirname(state.data.files[fileIndex].path);
        // Drill to the nearest ancestor directory that exists as a node and
        // contains the file, so the selection is visible.
        if (
          !state.data.files[fileIndex].path.startsWith(
            state.drillPath === "" ? "" : `${state.drillPath}/`,
          )
        ) {
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
    // Focus follows the panel: opening a selection focuses its close
    // button (after the render rAF builds the DOM); closing returns
    // focus to the canvas so keyboard users are never stranded.
    if (fileIndex !== null) {
      requestAnimationFrame(() => {
        panel.querySelector<HTMLButtonElement>("button.close")?.focus();
      });
    } else if (hadSelection) {
      canvas.focus();
    }
  };

  // ── Render loop (rAF-coalesced) ───────────────────────────────
  let renderQueued = false;
  // null forces the very first render to build the panel.
  let renderedPanelKey: string | null = null;
  const requestRender = (): void => {
    if (renderQueued) return;
    renderQueued = true;
    requestAnimationFrame(() => {
      renderQueued = false;
      rerenderChrome();
      // Rebuilding the panel DOM on every rAF is wasteful and destroys
      // transient widget state (the copy button's "copied" flash), so
      // it only runs when panel-relevant state actually changed.
      const panelKey = panelRenderKey(state);
      if (panelKey !== renderedPanelKey) {
        renderedPanelKey = panelKey;
        renderPanel(
          state,
          panel,
          (fileIndex) => selectFile(fileIndex, true),
          () => {
            state.selectedRoad = null;
            selectFile(null);
          },
          requestRender,
        );
      }
      renderView(state);
      syncHash(state);
    });
  };
  state.requestRender = requestRender;

  // ── Canvas interactions ───────────────────────────────────────
  let lastGraphTarget = "";

  const canvasPoint = (event: MouseEvent): { x: number; y: number } => {
    const rect = canvas.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  };

  const handleMapHover = (event: MouseEvent, x: number, y: number): void => {
    const hit = treemapHitTest(state, x, y);
    if (hit !== state.hoveredCell) {
      state.hoveredCell = hit;
      requestRender();
    }
    if (hit === null) {
      canvas.style.cursor = "default";
      hideTooltip();
      return;
    }
    const cell = state.layout[hit];
    canvas.style.cursor = "pointer";
    if (cell.node.fileIndex !== null) {
      showFileTooltip(state, cell.node.fileIndex, event.clientX, event.clientY);
    } else {
      showDirTooltip(
        cell.node.name,
        countLeaves(cell.node),
        cell.node.size,
        countLensFindings(state, cell.node),
        event.clientX,
        event.clientY,
      );
    }
  };

  const handleGraphHover = (event: MouseEvent, x: number, y: number): void => {
    const target = graphHoverTarget(state, x, y);
    const hovered = target && target.kind === "file" ? target.fileIndex : null;
    const targetKey = target
      ? target.kind === "road"
        ? `road:${target.road}`
        : target.kind === "file"
          ? `file:${target.fileIndex}`
          : target.kind === "cluster"
            ? `cluster:${target.cluster}`
            : "ui"
      : "";
    if (hovered !== state.graphHovered || targetKey !== lastGraphTarget) {
      state.graphHovered = hovered;
      lastGraphTarget = targetKey;
      renderGraph(state);
    }
    canvas.style.cursor = target ? "pointer" : state.selected !== null ? "default" : "grab";
    if (hovered !== null && hovered !== state.selected) {
      // In the graph the tooltip docks to the far canvas edge, so it
      // never covers the hovered neighborhood; ego mode keeps the
      // cursor-following variant for its list rows.
      const pos = state.selected === null ? nodeScreenPos(state, hovered) : null;
      const rect = canvas.getBoundingClientRect();
      showFileTooltip(
        state,
        hovered,
        event.clientX,
        event.clientY,
        pos
          ? {
              nodeX: pos.x,
              nodeY: pos.y,
              canvas: rect,
              usableW: usableStageWidth(state, rect.width),
            }
          : undefined,
      );
    } else if (target && target.kind === "road") {
      const facts = roadFacts(state, target.road);
      showRoadTooltip(
        facts.srcKey,
        facts.dstKey,
        facts.count,
        facts.violations,
        facts.cycleEdges,
        event.clientX,
        event.clientY,
      );
    } else {
      hideTooltip();
    }
  };

  canvas.addEventListener("mousemove", (event) => {
    const { x, y } = canvasPoint(event);
    if (state.view === "map") handleMapHover(event, x, y);
    else handleGraphHover(event, x, y);
  });

  canvas.addEventListener("mouseleave", () => {
    hideTooltip();
    if (state.view === "map" && state.hoveredCell !== null) {
      state.hoveredCell = null;
      requestRender();
    }
    if (state.view === "graph") {
      const changed = state.graphHovered !== null;
      state.graphHovered = null;
      lastGraphTarget = "";
      if (clearRoadHover(state) || changed) renderGraph(state);
    }
  });

  canvas.addEventListener("mousedown", (event) => {
    if (event.button === 0) dismissIntro(state);
  });

  // Selection runs on the native `click`, not mouseup. d3-zoom owns the drag
  // gesture (so the user can pan by dragging from ANYWHERE, including a node or
  // road) and, via clickDistance, lets a genuine click's `click` event through
  // while suppressing it after a real drag. So a click here is always a click,
  // never a pan.
  canvas.addEventListener("click", (event) => {
    if (event.button !== 0) return;
    const { x, y } = canvasPoint(event);
    if (state.view === "graph" && minimapHit(state, x, y)) {
      minimapPan(state, x, y);
      return;
    }
    if (state.view === "map") {
      const hit = treemapHitTest(state, x, y);
      if (hit === null) return;
      const cell = state.layout[hit];
      if (cell.node.fileIndex !== null) {
        hideTooltip();
        selectFile(cell.node.fileIndex);
      } else {
        hideTooltip();
        drillInto(state, cell);
        requestRender();
      }
      return;
    }
    if (event.shiftKey && graphPathTrace(state, x, y)) return;
    const result = graphHandleClick(state, x, y);
    if (result.kind === "file") {
      state.selectedRoad = null;
      hideTooltip();
      selectFile(result.fileIndex);
    } else if (result.kind === "road") {
      state.selectedRoad = result.road;
      state.selected = null;
      hideTooltip();
      requestRender();
    } else if (result.kind === "none") {
      state.selectedRoad = null;
      selectFile(null);
    } else {
      requestRender();
    }
  });

  // ── Keyboard ──────────────────────────────────────────────────
  const lensOrder: Lens[] = ["overview", "deadcode", "dupes", "boundaries", "hotspots"];
  /** One step back per press: help, search, selection, drill, lens. */
  const handleEscape = (inInput: boolean): void => {
    if (state.helpOpen) {
      toggleHelp();
      return;
    }
    if (inInput && refs) {
      refs.search.value = "";
      runSearch(state, "");
      refs.search.blur();
      requestRender();
      return;
    }
    if (state.selected !== null) {
      selectFile(null);
    } else if (state.selectedClone !== null) {
      state.selectedClone = null;
      requestRender();
    } else if (state.selectedRoad !== null || clearGraphFocus(state)) {
      state.selectedRoad = null;
      requestRender();
    } else if (state.search !== "" && refs) {
      refs.search.value = "";
      runSearch(state, "");
      requestRender();
    } else if (state.view === "map" && drillUp(state)) {
      requestRender();
    } else if (state.lens !== "overview") {
      setLens("overview");
    }
  };

  window.addEventListener("keydown", (event) => {
    const target = event.target as HTMLElement | null;
    const inInput = target?.tagName === "INPUT" || target?.tagName === "TEXTAREA";

    if (event.key === "Escape") {
      handleEscape(inInput);
      return;
    }
    if (inInput) {
      if (event.key === "Enter" && state.view === "graph" && state.search.trim() !== "") {
        graphFocusSearch(state);
      }
      return;
    }

    if (state.helpOpen) {
      // With the modal open, only "?" (toggle closed) acts; lens/view
      // shortcuts must not mutate the map behind the dialog.
      if (event.key === "?") toggleHelp();
      return;
    }

    if (event.key === "/") {
      event.preventDefault();
      refs?.search.focus();
    } else if (event.key === "?") {
      toggleHelp();
    } else if (event.key >= "1" && event.key <= "5") {
      setLens(lensOrder[Number(event.key) - 1]);
    } else if (event.key === "t" || event.key === "m") {
      setView("map");
    } else if (event.key === "g") {
      setView("graph");
    } else if (event.key === "0") {
      if (state.view === "graph") {
        resetGraphView(state);
      } else {
        drillTo(state, "");
        requestRender();
      }
    }
  });

  window.addEventListener("resize", () => {
    if (state.view === "graph") refitOnResize(state);
    requestRender();
  });
  // Track OS-level motion preference changes mid-session.
  window.matchMedia("(prefers-reduced-motion: reduce)").addEventListener("change", (event) => {
    state.reducedMotion = event.matches;
    requestRender();
  });
  window.addEventListener("hashchange", () => {
    applyHash(state, window.location.hash);
    requestRender();
  });

  // Initial paint.
  requestRender();

  // Keep the cluster segment in sync with the actual mode at boot.
  for (const [clusterMode, button] of refs.clusterButtons) {
    button.setAttribute("aria-pressed", String(clusterMode === getClusterMode(state)));
  }
};

const countLeaves = (node: {
  children: Array<{ children: unknown[]; fileIndex: number | null }>;
  fileIndex: number | null;
}): number => {
  if (node.fileIndex !== null) return 1;
  let count = 0;
  for (const child of node.children) {
    count += countLeaves(child as typeof node);
  }
  return count;
};

/** Count the active lens's findings under a treemap directory node. */
const countLensFindings = (
  state: AppState,
  node: TreeNode,
): { value: number; label: string } | null => {
  if (state.lens === "overview") return null;
  let value = 0;
  const walk = (current: TreeNode): void => {
    if (current.fileIndex !== null) {
      const file = state.data.files[current.fileIndex];
      switch (state.lens) {
        case "overview":
          break;
        case "deadcode":
          if (file.status === "unused" || file.status === "hasUnusedExports") value++;
          break;
        case "dupes":
          if (file.dup_lines > 0) value++;
          break;
        case "boundaries":
          if (state.index.violationSources.has(current.fileIndex)) value++;
          break;
        case "hotspots":
          if (file.max_cyclomatic >= 10) value++;
          break;
      }
      return;
    }
    for (const child of current.children) walk(child);
  };
  walk(node);
  const labels: Record<Lens, string> = {
    overview: "",
    deadcode: "unused",
    dupes: "with clones",
    boundaries: "violating",
    hotspots: "complex",
  };
  return { value, label: labels[state.lens] };
};

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", init);
} else {
  init();
}
