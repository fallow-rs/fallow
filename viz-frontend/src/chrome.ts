import type { AppState } from "./state";
import type { Lens } from "./types";
import { formatCount } from "./data";

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
    gloss: "cycles & layer breaks",
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

/** A labeled switch group: caps prefix + segmented rail. */
const switchGroup = (label: string): { group: HTMLElement; rail: HTMLElement } => {
  const group = el("div", "group");
  group.appendChild(el("span", "group-label", label));
  const rail = el("div", "seg");
  rail.setAttribute("role", "group");
  rail.setAttribute("aria-label", label.toLowerCase());
  group.appendChild(rail);
  return { group, rail };
};

// ── Build ───────────────────────────────────────────────────────

export const buildChrome = (
  state: AppState,
  app: HTMLElement,
  handlers: ChromeHandlers,
): ChromeRefs => {
  // Top bar: identity left, one-shot buttons right. No data boxes.
  const topbar = el("header");
  topbar.id = "topbar";

  const brand = el("div", "brand");
  brand.appendChild(el("span", "wordmark", "fallow"));
  brand.appendChild(el("span", "project", state.data.root));
  brand.appendChild(el("span", "cursor"));
  brand.appendChild(el("span", "sub", "codebase map"));
  topbar.appendChild(brand);

  const actions = el("div", "topbar-actions");
  const exportBtn = button("chip-btn", "png");
  exportBtn.id = "export-btn";
  exportBtn.title = "export this view as an image";
  exportBtn.setAttribute("aria-label", "Export the current view as PNG");
  actions.appendChild(exportBtn);
  const helpBtn = button("chip-btn", "?");
  helpBtn.id = "help-btn";
  helpBtn.title = "how to read this map";
  helpBtn.setAttribute("aria-label", "How to read this map");
  actions.appendChild(helpBtn);
  const themeToggle = button("chip-btn", state.dark ? "light" : "dark");
  themeToggle.id = "theme-toggle";
  themeToggle.title = "switch theme";
  themeToggle.setAttribute("aria-label", "Toggle color theme");
  themeToggle.addEventListener("click", handlers.onTheme);
  actions.appendChild(themeToggle);
  topbar.appendChild(actions);

  // Toolbar: three labeled switch groups + search.
  const toolbar = el("nav");
  toolbar.id = "toolbar";

  const viewGroup = switchGroup("VIEW");
  const viewButtons = new Map<string, HTMLButtonElement>();
  const viewDefs: Array<{ id: "graph" | "map"; name: string; gloss: string }> = [
    { id: "graph", name: "graph", gloss: "folders connected by imports" },
    { id: "map", name: "treemap", gloss: "nested boxes sized by file size" },
  ];
  for (const def of viewDefs) {
    const b = button("", def.name);
    b.title = def.gloss;
    b.setAttribute("aria-pressed", String(state.view === def.id));
    b.addEventListener("click", () => handlers.onView(def.id));
    viewButtons.set(def.id, b);
    viewGroup.rail.appendChild(b);
  }
  toolbar.appendChild(viewGroup.group);

  const lensGroup = switchGroup("LENS");
  const lensButtons = new Map<Lens, HTMLButtonElement>();
  for (const def of LENSES) {
    const b = button("lens-tab", "");
    const nameLine = el("span", "seg-name", def.name);
    const badge = el("span", "badge");
    nameLine.appendChild(badge);
    b.appendChild(nameLine);
    b.appendChild(el("span", "gloss", def.gloss));
    b.setAttribute("aria-pressed", String(state.lens === def.id));
    b.addEventListener("click", () => handlers.onLens(def.id));
    lensButtons.set(def.id, b);
    lensGroup.rail.appendChild(b);
  }
  toolbar.appendChild(lensGroup.group);

  const clusterGroupParts = switchGroup("ARRANGE");
  const clusterButtons = new Map<string, HTMLButtonElement>();
  const clusterDefs: Array<{ id: "directory" | "imports"; name: string; gloss: string }> = [
    { id: "directory", name: "by folder", gloss: "group files by their folder" },
    { id: "imports", name: "by imports", gloss: "group files that import each other" },
  ];
  for (const def of clusterDefs) {
    const b = button("", def.name);
    b.title = def.gloss;
    b.setAttribute("aria-pressed", String(def.id === "directory"));
    b.addEventListener("click", () => handlers.onCluster(def.id));
    clusterButtons.set(def.id, b);
    clusterGroupParts.rail.appendChild(b);
  }
  clusterGroupParts.group.classList.add("cluster-group");
  toolbar.appendChild(clusterGroupParts.group);

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

  // One dim line that says what the active lens just did.
  const summaryLine = el("div");
  summaryLine.id = "lens-summary";

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
    clusterGroup: clusterGroupParts.group,
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
    const badge = b.querySelector(".badge");
    if (badge) {
      const count = def.count(state);
      // A silent badge is ambiguous ("clean, or not counted?"): finding
      // lenses always show their number, with zero in a calm green.
      const unit = count !== null && count > 0 ? BADGE_UNITS[def.id] : "";
      badge.textContent = count !== null ? `${formatCount(count)}${unit}` : "";
      badge.className = count === 0 ? "badge sev-zero" : `badge sev-${def.sev}`;
    }
  }
  refs.clusterGroup.style.display = state.view === "graph" ? "" : "none";
  refs.themeToggle.textContent = state.dark ? "light" : "dark";

  updateSummaryLine(state, refs);
  updateCrumbs(state, refs);
  updateStatusInfo(state, refs);
  updateSearchCount(state, refs);
};

/** Unit suffix where a bare badge number invites the wrong reading. */
const BADGE_UNITS: Record<Lens, string> = {
  overview: "",
  deadcode: "",
  dupes: " groups",
  boundaries: "",
  hotspots: " files",
};

/** The CLI that reproduces the active lens's findings in the terminal. */
const LENS_COMMANDS: Record<Lens, string> = {
  overview: "",
  deadcode: "$ fallow dead-code",
  dupes: "$ fallow dupes",
  boundaries: "$ fallow dead-code --circular-deps --boundary-violations",
  hotspots: "$ fallow health",
};

/** Plain-language line under the rail: what did switching this lens do? */
const updateSummaryLine = (state: AppState, refs: ChromeRefs): void => {
  const s = state.data.summary;
  let text = "";
  switch (state.lens) {
    case "overview":
      text = "";
      break;
    case "deadcode":
      text =
        s.unused_files + s.unused_exports === 0
          ? "nothing unreachable · every file is reachable from an entry point"
          : `${formatCount(s.unused_files)} files and ${formatCount(s.unused_exports)} exports nothing imports · shown red and amber`;
      break;
    case "dupes":
      text =
        s.clone_groups === 0
          ? "no duplicated blocks found"
          : state.selectedClone !== null
            ? `viewing one duplicated block of ${formatCount(s.clone_groups)} · esc returns to the list`
            : `${formatCount(s.clone_groups)} duplicated blocks (${formatCount(s.duplicated_lines)} lines) · deeper amber = more duplication · click a row in the list to see every copy`;
      break;
    case "boundaries":
      text =
        s.circular_deps + s.boundary_violations === 0
          ? "no import cycles or layer violations"
          : `${formatCount(s.circular_deps)} cycles · ${formatCount(s.boundary_violations)} layer breaks · shown as red and amber connections`;
      break;
    case "hotspots":
      text =
        s.hotspot_files === 0
          ? "no files in the top complexity band"
          : `${formatCount(s.hotspot_files)} files in the top complexity band · amber → red = harder to change safely`;
      break;
  }
  refs.summaryLine.replaceChildren();
  if (text !== "") {
    refs.summaryLine.appendChild(el("span", "summary-text", text));
    const cmd = LENS_COMMANDS[state.lens];
    if (cmd !== "") {
      const chip = button("summary-cmd", `run: ${cmd.slice(2)}`);
      chip.title = "copy this command";
      chip.addEventListener("click", () => {
        void navigator.clipboard?.writeText(cmd.slice(2)).then(() => {
          chip.textContent = "copied";
          setTimeout(() => {
            chip.textContent = `run: ${cmd.slice(2)}`;
          }, 1200);
        });
      });
      refs.summaryLine.appendChild(chip);
    }
  }
  refs.summaryLine.classList.toggle("visible", text !== "");
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
};
