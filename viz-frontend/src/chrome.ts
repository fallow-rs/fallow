import type { AppState } from "./state";
import type { Lens } from "./types";
import { formatCount } from "./data";
import { announce, button, copyButton, el } from "./dom";

/**
 * HTML chrome around the canvas. One rule carries the affordance story:
 * brackets mean pressable. Switch groups (VIEW / LENS / ARRANGE) get a
 * tiny caps prefix label; data (status line, captions) never wears
 * brackets. Finding counts live in exactly one place: the lens rail.
 */

export interface ChromeRefs {
  topbar: HTMLElement;
  toolbar: HTMLElement;
  search: HTMLInputElement;
  searchCount: HTMLElement;
  crumbs: HTMLElement;
  statusInfo: HTMLElement;
  themeToggle: HTMLButtonElement;
  viewButtons: Map<string, HTMLButtonElement>;
  lensButtons: Map<Lens, HTMLButtonElement>;
  clusterGroup: HTMLElement;
  clusterButtons: Map<string, HTMLButtonElement>;
  summaryLine: HTMLElement;
  /** Wired by main.ts after chrome build (breadcrumb navigation). */
  crumbHandler?: (path: string) => void;
  /** Wired by main.ts after chrome build (help overlay toggle). */
  helpHandler?: () => void;
  /** Wired by main.ts after chrome build (PNG export of the canvas). */
  exportHandler?: () => void;
  /** Wired by main.ts after chrome build (copy the whole-map markdown digest). */
  digestHandler?: () => void;
}

export interface ChromeHandlers {
  onView: (view: "map" | "graph") => void;
  onLens: (lens: Lens) => void;
  onSearch: (query: string) => void;
  onTheme: () => void;
  onCrumb: (path: string) => void;
  onCluster: (mode: "directory" | "imports") => void;
}

interface LensDef {
  id: Lens;
  name: string;
  gloss: string;
  /** Aggregated finding count for the badge; null hides the badge. */
  count: (state: AppState) => number | null;
  sev: "error" | "warn";
}

const LENSES: LensDef[] = [
  {
    id: "overview",
    name: "overview",
    gloss: "folders & imports",
    count: () => null,
    sev: "warn",
  },
  {
    id: "deadcode",
    name: "unused",
    gloss: "dead files & exports",
    count: (s) => s.data.summary.unused_files + s.data.summary.unused_exports,
    sev: "error",
  },
  {
    id: "dupes",
    name: "duplication",
    gloss: "copy-pasted code",
    count: (s) => s.data.summary.clone_groups,
    sev: "warn",
  },
  {
    id: "boundaries",
    name: "boundaries",
    gloss: "import loops & forbidden imports",
    count: (s) => s.data.summary.circular_deps + s.data.summary.boundary_violations,
    sev: "error",
  },
  {
    id: "hotspots",
    name: "complexity",
    gloss: "hardest files to change",
    count: (s) => s.data.summary.hotspot_files,
    sev: "warn",
  },
];

// ── Build ───────────────────────────────────────────────────────

export const buildChrome = (
  state: AppState,
  app: HTMLElement,
  handlers: ChromeHandlers,
): ChromeRefs => {
  // Row 1: identity left; search + quiet utility icons right.
  const topbar = el("header");
  topbar.id = "topbar";

  const brand = el("div", "brand");
  brand.appendChild(el("span", "wordmark", "fallow"));
  brand.appendChild(el("span", "project", state.data.root));
  brand.appendChild(el("span", "sub", "codebase map"));
  topbar.appendChild(brand);

  const actions = el("div", "topbar-actions");
  const searchWrap = el("div", "search-wrap");
  const search = document.createElement("input");
  search.id = "search";
  search.type = "search";
  search.placeholder = "search files…";
  search.setAttribute("aria-label", "Search files");
  search.addEventListener("input", () => handlers.onSearch(search.value));
  searchWrap.appendChild(search);
  searchWrap.appendChild(el("kbd", "search-key", "/"));
  actions.appendChild(searchWrap);
  const searchCount = el("span");
  searchCount.id = "search-count";
  // Match counts change only on the DOM side, so announce them politely.
  searchCount.setAttribute("role", "status");
  searchCount.setAttribute("aria-live", "polite");
  actions.appendChild(searchCount);
  actions.appendChild(el("span", "divider"));
  const digestBtn = button("ghost-btn", "copy map");
  digestBtn.id = "digest-btn";
  digestBtn.title = "copy a markdown digest of the whole map";
  digestBtn.setAttribute("aria-label", "Copy a markdown digest of the whole map");
  digestBtn.addEventListener("click", () => {
    refs.digestHandler?.();
    digestBtn.textContent = "copied";
    announce("copied the whole map as markdown");
    setTimeout(() => {
      digestBtn.textContent = "copy map";
    }, 1200);
  });
  actions.appendChild(digestBtn);
  const exportBtn = button("ghost-btn", "png");
  exportBtn.id = "export-btn";
  exportBtn.title = "export this view as an image";
  exportBtn.setAttribute("aria-label", "Export the current view as PNG");
  actions.appendChild(exportBtn);
  const helpBtn = button("ghost-btn", "?");
  helpBtn.id = "help-btn";
  helpBtn.title = "how to read this map";
  helpBtn.setAttribute("aria-label", "How to read this map");
  actions.appendChild(helpBtn);
  const themeToggle = button("ghost-btn", "◐");
  themeToggle.id = "theme-toggle";
  themeToggle.title = "switch theme";
  themeToggle.setAttribute("aria-label", "Toggle color theme");
  themeToggle.addEventListener("click", handlers.onTheme);
  actions.appendChild(themeToggle);
  topbar.appendChild(actions);

  // Row 2: the five lens tabs are the page's navigation; the view
  // toggle is a setting and reads as one. No section labels.
  const toolbar = el("nav");
  toolbar.id = "toolbar";

  const tabs = el("div", "lens-tabs");
  // Toolbar, not tablist: a canvas makes tabpanel semantics awkward, so
  // the rail is a group of pressable buttons (aria-pressed) with roving
  // arrow-key navigation, like the view segment.
  tabs.setAttribute("role", "toolbar");
  tabs.setAttribute("aria-label", "lens");
  const lensButtons = new Map<Lens, HTMLButtonElement>();
  LENSES.forEach((def, i) => {
    const b = button("lens-tab", "");
    b.appendChild(el("span", "tab-name", def.name));
    b.appendChild(el("span", "badge"));
    // Fold the 1-5 shortcut into the tooltip, as the view toggle does.
    b.title = `${def.gloss} · ${i + 1}`;
    b.setAttribute("aria-pressed", String(state.lens === def.id));
    // Roving tabindex: only the active tab sits in the tab order.
    b.tabIndex = state.lens === def.id ? 0 : -1;
    b.addEventListener("click", () => handlers.onLens(def.id));
    lensButtons.set(def.id, b);
    tabs.appendChild(b);
  });
  // Arrow keys move the lens selection, as toolbar semantics promise.
  tabs.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const ids = LENSES.map((d) => d.id);
    const active = ids.findIndex(
      (id) => lensButtons.get(id)?.getAttribute("aria-pressed") === "true",
    );
    const delta = e.key === "ArrowRight" ? 1 : -1;
    const next = ids[(active + delta + ids.length) % ids.length];
    handlers.onLens(next);
    lensButtons.get(next)?.focus();
  });
  toolbar.appendChild(tabs);

  const viewSeg = el("div", "seg view-seg");
  viewSeg.setAttribute("role", "group");
  viewSeg.setAttribute("aria-label", "view");
  const viewButtons = new Map<string, HTMLButtonElement>();
  const viewDefs: Array<{ id: "graph" | "map"; name: string; gloss: string }> = [
    { id: "graph", name: "graph", gloss: "folders connected by imports · g" },
    { id: "map", name: "treemap", gloss: "nested boxes sized by file size · t" },
  ];
  for (const def of viewDefs) {
    const b = button("", def.name);
    b.title = def.gloss;
    b.setAttribute("aria-pressed", String(state.view === def.id));
    b.addEventListener("click", () => handlers.onView(def.id));
    viewButtons.set(def.id, b);
    viewSeg.appendChild(b);
  }
  toolbar.appendChild(viewSeg);

  // Arrange lives in the context strip: it configures the graph canvas
  // and vanishes with it.
  const clusterGroup = el("div", "arrange");
  const clusterButtons = new Map<string, HTMLButtonElement>();
  const clusterDefs: Array<{ id: "directory" | "imports"; name: string; gloss: string }> = [
    { id: "directory", name: "by folder", gloss: "group files by their folder" },
    { id: "imports", name: "by imports", gloss: "group files that import each other" },
  ];
  clusterDefs.forEach((def, i) => {
    if (i > 0) clusterGroup.appendChild(el("span", "arrange-sep", "·"));
    const b = button("", def.name);
    b.title = def.gloss;
    b.setAttribute("aria-pressed", String(def.id === "directory"));
    b.addEventListener("click", () => handlers.onCluster(def.id));
    clusterButtons.set(def.id, b);
    clusterGroup.appendChild(b);
  });

  // One dim line that says what the active lens just did.
  const summaryLine = el("div");
  summaryLine.id = "lens-summary";
  const summaryLeft = el("div", "summary-left");
  summaryLine.appendChild(summaryLeft);
  summaryLine.appendChild(clusterGroup);

  app.appendChild(topbar);
  app.appendChild(toolbar);
  app.appendChild(summaryLine);

  // Status line (appended after the stage by main.ts).
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
    ["5", " lens"],
    ["g", " graph"],
    ["t", " treemap"],
    ["0", " reset"],
    ["esc", " back"],
    ["?", " help"],
  ];
  hintPairs.forEach(([key, label], i) => {
    if (i > 0) hints.appendChild(document.createTextNode(i === 2 ? "–" : " · "));
    hints.appendChild(el("b", undefined, key));
    if (label) hints.appendChild(document.createTextNode(label));
  });
  statusline.appendChild(hints);

  const refs: ChromeRefs = {
    topbar,
    toolbar,
    search,
    searchCount,
    crumbs,
    statusInfo,
    themeToggle,
    viewButtons,
    lensButtons,
    clusterGroup,
    clusterButtons,
    summaryLine,
  };

  helpBtn.addEventListener("click", () => refs.helpHandler?.());
  exportBtn.addEventListener("click", () => refs.exportHandler?.());

  return refs;
};

/** The statusline element is appended after the stage by main.ts. */
export const statuslineOf = (refs: ChromeRefs): HTMLElement => {
  const line = refs.crumbs.parentElement;
  if (!line) throw new Error("statusline detached");
  return line;
};

// ── Per-render updates ──────────────────────────────────────────

export const updateChrome = (state: AppState, refs: ChromeRefs): void => {
  for (const [view, b] of refs.viewButtons) {
    b.setAttribute("aria-pressed", String(state.view === view));
  }
  for (const def of LENSES) {
    const b = refs.lensButtons.get(def.id);
    if (!b) continue;
    b.setAttribute("aria-pressed", String(state.lens === def.id));
    b.tabIndex = state.lens === def.id ? 0 : -1;
    const badge = b.querySelector(".badge");
    if (badge) {
      const count = def.count(state);
      // Bare tabular numbers; the unit words live in the context strip.
      // Zero stays visible in muted ink so absence is never ambiguous.
      badge.textContent = count !== null ? formatCount(count) : "";
      // Severity drives the accent: error lenses count in red, warn in
      // amber, matching the red/amber meaning used across the map.
      const weight = state.lens === def.id ? "hot" : "warm";
      badge.className = count === 0 ? "badge zero" : `badge ${weight} ${def.sev}`;
    }
  }
  refs.clusterGroup.style.display = state.view === "graph" ? "" : "none";
  refs.themeToggle.title = state.dark ? "switch to light" : "switch to dark";

  updateSummaryLine(state, refs);
  updateCrumbs(state, refs);
  updateStatusInfo(state, refs);
  updateSearchCount(state, refs);
};

/** The CLI that reproduces the active lens's findings in the terminal. */
const LENS_COMMANDS: Record<Lens, string> = {
  overview: "",
  deadcode: "$ fallow dead-code",
  dupes: "$ fallow dupes",
  boundaries: "$ fallow dead-code --circular-deps --boundary-violations",
  hotspots: "$ fallow health",
};

/**
 * Context strip under the tabs, always on: the active lens's gloss
 * leads, then what it found, then the CLI that reproduces it. The
 * arrange toggle sits on the strip's right (graph view only).
 */
const updateSummaryLine = (state: AppState, refs: ChromeRefs): void => {
  const s = state.data.summary;
  const gloss = LENSES.find((d) => d.id === state.lens)?.gloss ?? "";
  let text = "";
  switch (state.lens) {
    case "overview":
      text = `${formatCount(s.total_files)} files · ${formatCount(s.total_edges)} imports`;
      break;
    case "deadcode":
      text =
        s.unused_files + s.unused_exports === 0
          ? "nothing unreachable · every file is reachable from an entry point"
          : `${formatCount(s.unused_files)} unused files and ${formatCount(s.unused_exports)} unused exports · shown red and amber`;
      break;
    case "dupes":
      text =
        s.clone_groups === 0
          ? "no duplicated blocks found"
          : state.selectedClone !== null
            ? `viewing one duplicated block of ${formatCount(s.clone_groups)} · esc returns to the list`
            : `${formatCount(s.clone_groups)} blocks (${formatCount(s.duplicated_lines)} lines) · click a row in the list to see every copy`;
      break;
    case "boundaries":
      text =
        s.circular_deps + s.boundary_violations === 0
          ? "no import loops or forbidden imports"
          : `${formatCount(s.circular_deps)} loop${s.circular_deps === 1 ? "" : "s"} · ${formatCount(s.boundary_violations)} forbidden import${s.boundary_violations === 1 ? "" : "s"} · shown as red and amber connections`;
      break;
    case "hotspots":
      text =
        s.hotspot_files === 0
          ? "no files flagged as complex"
          : `the ${formatCount(s.hotspot_files)} most complex files · amber → red = harder to change safely`;
      break;
  }
  const left = refs.summaryLine.querySelector(".summary-left");
  if (!left) return;
  left.replaceChildren();
  left.appendChild(el("span", "summary-gloss", gloss));
  left.appendChild(el("span", "summary-text", text));
  const cmd = LENS_COMMANDS[state.lens];
  if (cmd !== "") {
    const chip = copyButton("summary-cmd", `run: ${cmd.slice(2)}`, () => cmd.slice(2));
    chip.title = "copy this command";
    left.appendChild(chip);
  }
  refs.summaryLine.classList.add("visible");
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
      } else if (state.index.nodesByPath.has(acc)) {
        const target = acc;
        const b = button("", part);
        b.addEventListener("click", () => refs.crumbHandler?.(target));
        refs.crumbs.appendChild(b);
      } else {
        // Segment collapsed into a single-child directory chain: there is
        // no node to drill to, so render it as static text, not a dead link.
        refs.crumbs.appendChild(el("span", "collapsed", part));
      }
    });
  }
};

const updateStatusInfo = (state: AppState, refs: ChromeRefs): void => {
  const s = state.data.summary;
  const stamp = new Date(document.lastModified);
  const day = Number.isNaN(stamp.getTime()) ? "" : ` ${stamp.toISOString().slice(0, 10)}`;
  refs.statusInfo.textContent = `${formatCount(s.total_files)} files · ${formatCount(s.total_edges)} imports · generated${day} by fallow`;
};

const updateSearchCount = (state: AppState, refs: ChromeRefs): void => {
  if (state.search.trim() === "") {
    refs.searchCount.replaceChildren();
    return;
  }
  refs.searchCount.replaceChildren();
  const n = el("span", "n", formatCount(state.searchMatches.size));
  refs.searchCount.append(n, document.createTextNode(" matches"));
  if (state.searchReach.size > 0 && state.view === "graph") {
    const affects = el("span", "hint");
    affects.append(" · ", el("span", "n", formatCount(state.searchReach.size)), " downstream");
    refs.searchCount.appendChild(affects);
  } else if (state.searchMatches.size > 0 && state.view === "graph") {
    refs.searchCount.appendChild(el("span", "hint", " · enter zooms"));
  }
};
