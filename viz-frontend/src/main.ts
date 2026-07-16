import { createState } from "./state";
import { renderTreemap } from "./treemap";
import { initGraphNodes, renderGraph, setGraphFilter, setClusterMode, type GraphFilter, type ClusterMode } from "./graph";
import { setupInteractions } from "./interactions";
import { setupSearch } from "./search";
import { createSummaryBar } from "./summary";
import type { AppState } from "./state";

const renderActiveView = (state: AppState): void => {
  if (state.activeView === "treemap") {
    renderTreemap(state);
  } else {
    renderGraph(state);
  }
};

const init = (): void => {
  const data = window.__FALLOW_DATA__;
  if (!data) {
    document.body.textContent = "Error: No fallow visualization data found.";
    return;
  }

  // Root container
  const app = document.createElement("div");
  app.id = "fallow-app";
  document.body.appendChild(app);

  // Header: summary + controls
  const header = document.createElement("div");
  header.id = "fallow-header";
  app.appendChild(header);

  const summaryEl = createSummaryBar(data.summary, {
    bg: "#ffffff",
    text: "#1a1a1a",
    textSecondary: "#666",
    border: "rgba(0,0,0,0.15)",
    hover: "rgba(255,255,255,0.35)",
    directory: "#E8EDF2",
    directoryText: "#374151",
    statusColors: {
      clean: "#3B82F6",
      hasUnusedExports: "#F59E0B",
      unused: "#EF4444",
      entryPoint: "#10B981",
    },
    tooltipBg: "#ffffff",
    tooltipText: "#1a1a1a",
    tooltipBorder: "#e5e7eb",
    summaryBg: "#f9fafb",
    searchBg: "#ffffff",
    breadcrumbBg: "#f3f4f6",
    breadcrumbText: "#374151",
  });
  header.appendChild(summaryEl);

  // Controls row
  const controls = document.createElement("div");
  controls.id = "fallow-controls";
  header.appendChild(controls);

  // View toggle tabs
  const viewTabs = document.createElement("div");
  viewTabs.id = "fallow-view-tabs";
  viewTabs.setAttribute("role", "group");
  viewTabs.setAttribute("aria-label", "Visualization view");

  const treemapTab = document.createElement("button");
  treemapTab.className = "view-tab active";
  treemapTab.textContent = "Treemap";
  treemapTab.dataset.view = "treemap";
  treemapTab.setAttribute("aria-pressed", "true");
  viewTabs.appendChild(treemapTab);

  const graphTab = document.createElement("button");
  graphTab.className = "view-tab";
  graphTab.textContent = "Graph";
  graphTab.dataset.view = "graph";
  graphTab.setAttribute("aria-pressed", "false");
  viewTabs.appendChild(graphTab);

  controls.appendChild(viewTabs);

  // Graph filter buttons (visible only in graph view)
  const filterGroup = document.createElement("div");
  filterGroup.id = "fallow-graph-filters";
  filterGroup.style.display = "none";
  filterGroup.setAttribute("role", "group");
  filterGroup.setAttribute("aria-label", "Filter graph nodes");

  const filterButtons: Array<{ label: string; value: GraphFilter }> = [
    { label: "All", value: "all" },
    { label: "Unused", value: "unused" },
    { label: "Issues", value: "issues" },
  ];

  for (const fb of filterButtons) {
    const btn = document.createElement("button");
    const isActive = fb.value === "all";
    btn.className = `filter-btn${isActive ? " active" : ""}`;
    btn.textContent = fb.label;
    btn.dataset.filter = fb.value;
    btn.setAttribute("aria-pressed", String(isActive));
    btn.addEventListener("click", () => {
      for (const b of filterGroup.querySelectorAll(".filter-btn")) {
        const on = (b as HTMLElement).dataset.filter === fb.value;
        b.classList.toggle("active", on);
        b.setAttribute("aria-pressed", String(on));
      }
      setGraphFilter(state!, fb.value);
    });
    filterGroup.appendChild(btn);
  }
  controls.appendChild(filterGroup);

  // Cluster mode toggle (visible only in graph view)
  const clusterGroup = document.createElement("div");
  clusterGroup.id = "fallow-cluster-mode";
  clusterGroup.style.display = "none";
  clusterGroup.setAttribute("role", "group");
  clusterGroup.setAttribute("aria-label", "Graph clustering mode");

  const clusterLabel = document.createElement("span");
  clusterLabel.className = "cluster-label";
  clusterLabel.textContent = "Cluster:";
  clusterGroup.appendChild(clusterLabel);

  const clusterButtons: Array<{ label: string; value: ClusterMode }> = [
    { label: "Directory", value: "directory" },
    { label: "Imports", value: "imports" },
  ];

  for (const cb of clusterButtons) {
    const btn = document.createElement("button");
    const isActive = cb.value === "directory";
    btn.className = `filter-btn${isActive ? " active" : ""}`;
    btn.textContent = cb.label;
    btn.dataset.cluster = cb.value;
    btn.setAttribute("aria-pressed", String(isActive));
    btn.addEventListener("click", () => {
      for (const b of clusterGroup.querySelectorAll(".filter-btn")) {
        const on = (b as HTMLElement).dataset.cluster === cb.value;
        b.classList.toggle("active", on);
        b.setAttribute("aria-pressed", String(on));
      }
      setClusterMode(state!, cb.value);
    });
    clusterGroup.appendChild(btn);
  }
  controls.appendChild(clusterGroup);

  const breadcrumbEl = document.createElement("div");
  breadcrumbEl.id = "fallow-breadcrumb";
  controls.appendChild(breadcrumbEl);

  const canvas = document.createElement("canvas");
  canvas.id = "fallow-canvas";
  canvas.tabIndex = 0;
  canvas.setAttribute("role", "img");
  canvas.setAttribute(
    "aria-label",
    `Interactive visualization of ${data.summary.total_files} files. ` +
      "Use the Treemap and Graph buttons to switch views, the search box to find files, " +
      "and the number keys 1-3 to set graph focus depth.",
  );
  app.appendChild(canvas);

  const state = createState(data, canvas);
  if (!state) return;

  // Apply theme to body
  document.body.style.backgroundColor = state.theme.bg;
  document.body.style.color = state.theme.text;
  summaryEl.style.backgroundColor = state.theme.summaryBg;
  breadcrumbEl.style.backgroundColor = state.theme.breadcrumbBg;
  breadcrumbEl.style.color = state.theme.breadcrumbText;

  // Search
  const searchInput = setupSearch(state);
  controls.appendChild(searchInput);

  // Dark mode toggle
  const darkModeBtn = document.createElement("button");
  darkModeBtn.id = "fallow-darkmode";
  darkModeBtn.textContent = state.darkMode ? "☀" : "☾";
  darkModeBtn.title = "Toggle dark mode";
  darkModeBtn.setAttribute("aria-label", "Toggle dark mode");
  darkModeBtn.setAttribute("aria-pressed", String(state.darkMode));
  controls.appendChild(darkModeBtn);

  // Legend
  const legend = document.createElement("div");
  legend.id = "fallow-legend";
  legend.setAttribute("role", "list");
  legend.setAttribute("aria-label", "Status legend");
  const legendItems: Array<[string, string]> = [
    ["Clean", state.theme.statusColors.clean],
    ["Entry point", state.theme.statusColors.entryPoint],
    ["Unused exports", state.theme.statusColors.hasUnusedExports],
    ["Unused file", state.theme.statusColors.unused],
  ];
  for (const [label, color] of legendItems) {
    const item = document.createElement("span");
    item.className = "legend-item";
    item.setAttribute("role", "listitem");

    const swatch = document.createElement("span");
    swatch.className = "legend-swatch";
    swatch.style.backgroundColor = color;
    swatch.setAttribute("aria-hidden", "true");
    item.appendChild(swatch);

    const text = document.createElement("span");
    text.textContent = label;
    item.appendChild(text);

    legend.appendChild(item);
  }
  controls.appendChild(legend);

  // View tab switching
  let graphInitialized = false;

  const switchView = (view: "treemap" | "graph"): void => {
    state.activeView = view;
    treemapTab.classList.toggle("active", view === "treemap");
    graphTab.classList.toggle("active", view === "graph");
    treemapTab.setAttribute("aria-pressed", String(view === "treemap"));
    graphTab.setAttribute("aria-pressed", String(view === "graph"));
    breadcrumbEl.style.display = view === "treemap" ? "" : "none";
    filterGroup.style.display = view === "graph" ? "" : "none";
    clusterGroup.style.display = view === "graph" ? "" : "none";

    if (view === "graph" && !graphInitialized) {
      graphInitialized = true;
      initGraphNodes(state);
    } else {
      renderActiveView(state);
    }
  };

  treemapTab.addEventListener("click", () => switchView("treemap"));
  graphTab.addEventListener("click", () => switchView("graph"));

  // Initial render
  renderTreemap(state);

  // Wire up interactions
  setupInteractions(state, breadcrumbEl, searchInput, summaryEl, darkModeBtn, renderActiveView);
};

// Boot
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", init);
} else {
  init();
}
