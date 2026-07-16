import type { AppState } from "./state";
import type { Lens } from "./types";
import { dupRamp, heatRamp } from "./theme";
import { formatCount } from "./data";

export interface ChromeRefs {
  topbar: HTMLElement;
  toolbar: HTMLElement;
  stats: HTMLElement;
  legend: HTMLElement;
  search: HTMLInputElement;
  searchCount: HTMLElement;
  crumbs: HTMLElement;
  statusInfo: HTMLElement;
  themeToggle: HTMLButtonElement;
  viewButtons: Map<string, HTMLButtonElement>;
  lensButtons: Map<Lens, HTMLButtonElement>;
  clusterSeg: HTMLElement;
  clusterButtons: Map<string, HTMLButtonElement>;
  emptyState: HTMLElement;
  /** Wired by main.ts after chrome build (breadcrumb navigation). */
  crumbHandler?: (path: string) => void;
  /** Wired by main.ts after chrome build (help overlay toggle). */
  helpHandler?: () => void;
}

export interface ChromeHandlers {
  onView: (view: "map" | "graph") => void;
  onLens: (lens: Lens) => void;
  onSearch: (query: string) => void;
  onTheme: () => void;
  onCrumb: (path: string) => void;
  onCluster: (mode: "directory" | "imports") => void;
}

const el = (tag: string, cls?: string, text?: string): HTMLElement => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
};

const button = (cls: string, text: string): HTMLButtonElement => {
  const b = document.createElement("button");
  b.type = "button";
  b.className = cls;
  b.textContent = text;
  return b;
};

const LENS_LABELS: Record<Lens, string> = {
  deadcode: "dead code",
  dupes: "duplication",
  boundaries: "boundaries",
  hotspots: "hotspots",
};

// ── Build ───────────────────────────────────────────────────────

export const buildChrome = (
  state: AppState,
  app: HTMLElement,
  handlers: ChromeHandlers,
): ChromeRefs => {
  // Top bar
  const topbar = el("header");
  topbar.id = "topbar";

  const brand = el("div", "brand");
  brand.appendChild(el("span", "wordmark", "fallow"));
  brand.appendChild(el("span", "project", state.data.root));
  brand.appendChild(el("span", "cursor"));
  brand.appendChild(el("span", "sub", "codebase map"));
  topbar.appendChild(brand);

  const stats = el("div");
  stats.id = "stats";
  topbar.appendChild(stats);

  const helpBtn = button("", "?");
  helpBtn.id = "help-btn";
  helpBtn.setAttribute("aria-label", "How to read this map");
  topbar.appendChild(helpBtn);

  const themeToggle = button("", state.dark ? "light" : "dark");
  themeToggle.id = "theme-toggle";
  themeToggle.setAttribute("aria-label", "Toggle color theme");
  themeToggle.addEventListener("click", handlers.onTheme);
  topbar.appendChild(themeToggle);

  // Toolbar
  const toolbar = el("nav");
  toolbar.id = "toolbar";

  const viewSeg = el("div", "seg");
  viewSeg.setAttribute("role", "group");
  viewSeg.setAttribute("aria-label", "View");
  const viewButtons = new Map<string, HTMLButtonElement>();
  for (const view of ["map", "graph"] as const) {
    const b = button("", view);
    b.setAttribute("aria-pressed", String(state.view === view));
    b.addEventListener("click", () => handlers.onView(view));
    viewButtons.set(view, b);
    viewSeg.appendChild(b);
  }
  toolbar.appendChild(viewSeg);

  const lensSeg = el("div", "seg");
  lensSeg.setAttribute("role", "group");
  lensSeg.setAttribute("aria-label", "Lens");
  const lensButtons = new Map<Lens, HTMLButtonElement>();
  for (const lens of ["deadcode", "dupes", "boundaries", "hotspots"] as const) {
    const b = button("", LENS_LABELS[lens]);
    b.setAttribute("aria-pressed", String(state.lens === lens));
    const badge = el("span", "badge");
    b.appendChild(badge);
    b.addEventListener("click", () => handlers.onLens(lens));
    lensButtons.set(lens, b);
    lensSeg.appendChild(b);
  }
  toolbar.appendChild(lensSeg);

  // Graph cluster mode (visible in graph view only)
  const clusterSeg = el("div", "seg");
  clusterSeg.setAttribute("role", "group");
  clusterSeg.setAttribute("aria-label", "Graph clustering");
  clusterSeg.style.display = "none";
  const clusterButtons = new Map<string, HTMLButtonElement>();
  for (const mode of ["directory", "imports"] as const) {
    const b = button("", mode === "directory" ? "by folder" : "by imports");
    b.setAttribute("aria-pressed", String(mode === "directory"));
    b.addEventListener("click", () => handlers.onCluster(mode));
    clusterButtons.set(mode, b);
    clusterSeg.appendChild(b);
  }
  toolbar.appendChild(clusterSeg);

  const search = document.createElement("input");
  search.id = "search";
  search.type = "search";
  search.placeholder = "/ search files";
  search.setAttribute("aria-label", "Search files");
  search.addEventListener("input", () => handlers.onSearch(search.value));
  toolbar.appendChild(search);

  const searchCount = el("span");
  searchCount.id = "search-count";
  toolbar.appendChild(searchCount);

  const legend = el("div");
  legend.id = "legend";
  legend.setAttribute("role", "list");
  legend.setAttribute("aria-label", "Legend");
  toolbar.appendChild(legend);

  app.appendChild(topbar);
  app.appendChild(toolbar);

  // Stage + statusline are built in main; crumbs/status live in the footer.
  const statusline = el("footer");
  statusline.id = "statusline";
  const crumbs = el("div");
  crumbs.id = "crumbs";
  statusline.appendChild(crumbs);
  const statusInfo = el("span");
  statusInfo.id = "status-info";
  statusline.appendChild(statusInfo);
  const hints = el("span");
  hints.id = "hints";
  const hintPairs: Array<[string, string]> = [
    ["/", " search"],
    ["1", ""],
    ["4", " lens"],
    ["m", " map"],
    ["g", " graph"],
    ["esc", " back"],
  ];
  hintPairs.forEach(([key, label], i) => {
    if (i > 0) hints.appendChild(document.createTextNode(i === 2 ? "–" : " · "));
    hints.appendChild(el("b", undefined, key));
    if (label) hints.appendChild(document.createTextNode(label));
  });
  statusline.appendChild(hints);

  const emptyState = el("div");
  emptyState.id = "empty-state";

  const refs: ChromeRefs = {
    topbar,
    toolbar,
    stats,
    legend,
    search,
    searchCount,
    crumbs,
    statusInfo,
    themeToggle,
    viewButtons,
    lensButtons,
    clusterSeg,
    clusterButtons,
    emptyState,
  };

  helpBtn.addEventListener("click", () => refs.helpHandler?.());

  buildStats(state, refs, handlers);
  return refs;
};

/** The statusline element is appended after the stage by main.ts. */
export const statuslineOf = (refs: ChromeRefs): HTMLElement => {
  const line = refs.crumbs.parentElement;
  if (!line) throw new Error("statusline detached");
  return line;
};

// ── Stat boxes ──────────────────────────────────────────────────

interface StatDef {
  label: string;
  value: number;
  sev: "error" | "warn" | "ok" | "plain";
  lens: Lens | null;
}

const buildStats = (state: AppState, refs: ChromeRefs, handlers: ChromeHandlers): void => {
  const s = state.data.summary;
  const defs: StatDef[] = [
    { label: "files", value: s.total_files, sev: "plain", lens: null },
    { label: "unused files", value: s.unused_files, sev: "error", lens: "deadcode" },
    { label: "unused exports", value: s.unused_exports, sev: "warn", lens: "deadcode" },
    { label: "clones", value: s.clone_groups, sev: "warn", lens: "dupes" },
    { label: "cycles", value: s.circular_deps, sev: "warn", lens: "deadcode" },
    { label: "violations", value: s.boundary_violations, sev: "error", lens: "boundaries" },
    { label: "hotspots", value: s.hotspot_files, sev: "warn", lens: "hotspots" },
  ];

  refs.stats.replaceChildren();
  defs.forEach((def, i) => {
    const stat = button("stat", "");
    if (def.value === 0) stat.classList.add("zero");
    else if (def.sev !== "plain") stat.classList.add(`sev-${def.sev}`);
    stat.appendChild(el("span", "bracket", "[ "));
    stat.appendChild(el("span", "label", `${def.label} `));
    stat.appendChild(el("span", "value", formatCount(def.value)));
    stat.appendChild(el("span", "bracket", " ]"));
    if (def.lens) {
      stat.title = `open the ${LENS_LABELS[def.lens]} lens`;
      stat.addEventListener("click", () => {
        if (def.lens) handlers.onLens(def.lens);
      });
    } else {
      stat.disabled = true;
      stat.style.cursor = "default";
    }
    refs.stats.appendChild(stat);
    // Staggered terminal-style reveal.
    const delay = state.reducedMotion ? 0 : 60 + i * 45;
    window.setTimeout(() => stat.classList.add("revealed"), delay);
  });
};

// ── Per-render updates ──────────────────────────────────────────

export const updateChrome = (state: AppState, refs: ChromeRefs): void => {
  for (const [view, b] of refs.viewButtons) {
    b.setAttribute("aria-pressed", String(state.view === view));
  }
  for (const [lens, b] of refs.lensButtons) {
    b.setAttribute("aria-pressed", String(state.lens === lens));
    const badge = b.querySelector(".badge");
    if (badge) {
      const count = lensFindingCount(state, lens);
      badge.textContent = count > 0 ? formatCount(count) : "";
      badge.className = `badge ${count === 0 ? "zero" : lens === "deadcode" || lens === "boundaries" ? "sev-error" : "sev-warn"}`;
    }
  }
  refs.clusterSeg.style.display = state.view === "graph" ? "" : "none";
  refs.themeToggle.textContent = state.dark ? "light" : "dark";

  updateLegend(state, refs);
  updateCrumbs(state, refs);
  updateStatusInfo(state, refs);
  updateSearchCount(state, refs);
  updateEmptyState(state, refs);
};

const lensFindingCount = (state: AppState, lens: Lens): number => {
  const s = state.data.summary;
  switch (lens) {
    case "deadcode":
      return s.unused_files + s.unused_exports + s.circular_deps;
    case "dupes":
      return s.clone_groups;
    case "boundaries":
      return s.boundary_violations;
    case "hotspots":
      return s.hotspot_files;
  }
};

const legendItem = (swatchColor: string, label: string, hatched = false): HTMLElement => {
  const item = el("span", "legend-item");
  item.setAttribute("role", "listitem");
  const swatch = el("span", "legend-swatch");
  swatch.style.background = hatched
    ? `repeating-linear-gradient(45deg, ${swatchColor}, ${swatchColor} 2px, transparent 2px, transparent 4px)`
    : swatchColor;
  item.appendChild(swatch);
  item.appendChild(el("span", undefined, label));
  return item;
};

const rampEl = (stops: string[]): HTMLElement => {
  const ramp = el("span", "legend-ramp");
  ramp.style.background = `linear-gradient(to right, ${stops.join(", ")})`;
  return ramp;
};

const updateLegend = (state: AppState, refs: ChromeRefs): void => {
  const t = state.theme;
  refs.legend.replaceChildren();
  switch (state.lens) {
    case "deadcode": {
      refs.legend.append(
        legendItem(t.red, "unused file", true),
        legendItem(t.amber, "unused exports"),
        legendItem(t.cellEntry, "entry point"),
        legendItem(t.cellNeutral, "live"),
      );
      break;
    }
    case "dupes": {
      const item = el("span", "legend-item");
      item.appendChild(el("span", undefined, "0%"));
      item.appendChild(rampEl([t.cellNeutral, dupRamp(t, 0.5), dupRamp(t, 1)]));
      item.appendChild(el("span", undefined, "duplicated lines"));
      refs.legend.append(item);
      break;
    }
    case "boundaries": {
      const zones = state.data.zones;
      zones.slice(0, t.zones.length).forEach((zone, i) => {
        refs.legend.appendChild(legendItem(t.zones[i], zone.name));
      });
      if (zones.length > t.zones.length) {
        refs.legend.appendChild(legendItem(t.zoneOther, "other zones"));
      }
      if (zones.length > 0) {
        refs.legend.appendChild(legendItem(t.red, "violation", true));
      }
      break;
    }
    case "hotspots": {
      const item = el("span", "legend-item");
      item.appendChild(el("span", undefined, "cc 1"));
      item.appendChild(rampEl([t.cellNeutral, heatRamp(t, 0.45), heatRamp(t, 1)]));
      item.appendChild(el("span", undefined, `${state.index.heatCeiling}+`));
      refs.legend.append(item);
      break;
    }
  }
};

const updateCrumbs = (state: AppState, refs: ChromeRefs): void => {
  refs.crumbs.replaceChildren();
  if (state.view !== "map") {
    refs.crumbs.appendChild(el("span", "current", "import graph"));
    return;
  }
  const rootBtn = button("", state.data.root);
  rootBtn.addEventListener("click", () => refs.crumbHandler?.(""));
  refs.crumbs.appendChild(rootBtn);

  if (state.drillPath !== "") {
    const parts = state.drillPath.split("/");
    let acc = "";
    parts.forEach((part, i) => {
      refs.crumbs.appendChild(el("span", "sep", "/"));
      acc = acc ? `${acc}/${part}` : part;
      if (i === parts.length - 1) {
        refs.crumbs.appendChild(el("span", "current", part));
      } else {
        const target = acc;
        const b = button("", part);
        b.addEventListener("click", () => refs.crumbHandler?.(target));
        refs.crumbs.appendChild(b);
      }
    });
  }
};

const updateStatusInfo = (state: AppState, refs: ChromeRefs): void => {
  const s = state.data.summary;
  refs.statusInfo.textContent = `${formatCount(s.total_files)} files · ${formatCount(s.total_edges)} imports · generated by fallow`;
};

const updateSearchCount = (state: AppState, refs: ChromeRefs): void => {
  if (state.search.trim() === "") {
    refs.searchCount.replaceChildren();
    return;
  }
  refs.searchCount.replaceChildren();
  const n = el("span", "n", formatCount(state.searchMatches.size));
  refs.searchCount.append(n, document.createTextNode(" matches"));
};

const updateEmptyState = (state: AppState, refs: ChromeRefs): void => {
  const show = state.lens === "boundaries" && state.data.zones.length === 0;
  refs.emptyState.classList.toggle("visible", show);
  if (show && refs.emptyState.childElementCount === 0) {
    const box = el("div", "box");
    box.appendChild(el("div", "title", "no boundary zones configured"));
    const body = el("div");
    body.append(
      "Define architecture zones in ",
      codeEl(".fallowrc.json"),
      " under ",
      codeEl("boundaries.zones"),
      " (or pick a preset like ",
      codeEl("layered"),
      ") and fallow will map every file to a zone and flag imports that cross the rules.",
    );
    box.appendChild(body);
    refs.emptyState.appendChild(box);
  }
};

const codeEl = (text: string): HTMLElement => {
  const code = document.createElement("code");
  code.textContent = text;
  return code;
};
