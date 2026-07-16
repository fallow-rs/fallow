import type { AppState } from "./state";
import type { Insight, InsightAction } from "./insights";
import { formatCount } from "./data";

/**
 * HTML layers over the canvas: the briefing drawer (auto-generated
 * orientation insights), the help overlay (how to read the map), and
 * the presenter tour bar. All DOM, all styled by the shared tokens.
 */

export interface OverlayHandlers {
  onInsight: (action: InsightAction) => void;
  onTourStart: () => void;
  onTourStep: (delta: number) => void;
  onTourEnd: () => void;
  onHelpClose: () => void;
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

const bracketTitle = (host: HTMLElement, title: string): void => {
  host.appendChild(el("span", "bracket", "[ "));
  host.appendChild(document.createTextNode(title));
  host.appendChild(el("span", "bracket", " ]"));
};

// ── Briefing drawer ─────────────────────────────────────────────

export const buildBriefing = (
  state: AppState,
  insights: Insight[],
  handlers: OverlayHandlers,
): { briefing: HTMLElement; toggle: HTMLButtonElement } => {
  const briefing = el("aside");
  briefing.id = "briefing";
  briefing.setAttribute("aria-label", "Codebase briefing");

  const head = el("div", "brief-head");
  const title = el("span", "brief-title");
  bracketTitle(title, "briefing");
  head.appendChild(title);
  const collapse = button("brief-collapse", "–");
  collapse.setAttribute("aria-label", "Collapse briefing");
  head.appendChild(collapse);
  briefing.appendChild(head);

  const list = el("ol", "brief-list");
  insights.forEach((insight) => {
    const li = el("li");
    const btn = button("brief-item", "");
    btn.appendChild(el("span", `brief-dot sev-${insight.severity}`));
    const body = el("span", "brief-body");
    body.appendChild(el("span", "brief-text", insight.text));
    if (insight.detail) body.appendChild(el("span", "brief-detail", insight.detail));
    btn.appendChild(body);
    btn.addEventListener("click", () => handlers.onInsight(insight.action));
    li.appendChild(btn);
    list.appendChild(li);
  });
  if (insights.length === 0) {
    const li = el("li", "brief-empty", "no findings to brief; this map is clean");
    list.appendChild(li);
  }
  briefing.appendChild(list);

  const foot = el("div", "brief-foot");
  if (insights.length > 1) {
    const tour = button("brief-tour", "▸ start tour");
    tour.addEventListener("click", handlers.onTourStart);
    foot.appendChild(tour);
  }
  foot.appendChild(el("span", "brief-hint", "b toggle · ? help"));
  briefing.appendChild(foot);

  const toggle = button("brief-toggle", "");
  toggle.id = "briefing-toggle";
  toggle.appendChild(el("span", "bracket", "[ "));
  toggle.appendChild(document.createTextNode("briefing "));
  toggle.appendChild(el("span", "value", formatCount(insights.length)));
  toggle.appendChild(el("span", "bracket", " ]"));
  toggle.setAttribute("aria-label", "Open briefing");

  const sync = (): void => {
    briefing.classList.toggle("open", state.briefingOpen);
    toggle.classList.toggle("hidden", state.briefingOpen);
  };
  collapse.addEventListener("click", () => {
    state.briefingOpen = false;
    sync();
  });
  toggle.addEventListener("click", () => {
    state.briefingOpen = true;
    sync();
  });
  sync();

  return { briefing, toggle };
};

/** Re-sync open/collapsed classes after programmatic state changes. */
export const syncBriefing = (
  state: AppState,
  briefing: HTMLElement,
  toggle: HTMLElement,
): void => {
  briefing.classList.toggle("open", state.briefingOpen);
  toggle.classList.toggle("hidden", state.briefingOpen);
};

// ── Help overlay ────────────────────────────────────────────────

const HELP_SECTIONS: Array<{ title: string; rows: Array<[string, string]> }> = [
  {
    title: "views & lenses",
    rows: [
      ["map", "treemap of every file, sized by bytes, drill into folders"],
      ["graph", "import graph: entry code left, shared foundations right"],
      ["dead code", "red hatch = unused file · amber = unused exports"],
      ["duplication", "amber intensity = share of duplicated lines"],
      ["boundaries", "color = architecture zone · red = forbidden import"],
      ["hotspots", "amber to red = cyclomatic heat, React context included"],
    ],
  },
  {
    title: "reading the graph",
    rows: [
      ["roads", "bundled imports between folders, label = import count"],
      ["taper", "thick end = importer, thin end = imported"],
      ["red / amber road", "carries boundary violations / part of a cycle"],
      ["amber hull", "folder tangle: folders that import each other"],
      ["×N ring", "hub file, imported by N files"],
      ["standalone strip", "files no project code imports (configs, CI)"],
    ],
  },
  {
    title: "interactions",
    rows: [
      ["click file", "facts panel + importer/import columns"],
      ["click road", "list every file pair on that road"],
      ["shift-click ×2", "trace the shortest dependency path"],
      ["/ then enter", "search, zoom to the best match"],
      ["1 2 3 4", "switch lens · m map · g graph"],
      ["esc", "back out of anything"],
    ],
  },
];

export const buildHelpOverlay = (handlers: OverlayHandlers): HTMLElement => {
  const overlay = el("div");
  overlay.id = "help-overlay";
  overlay.setAttribute("role", "dialog");
  overlay.setAttribute("aria-label", "How to read this map");

  const box = el("div", "help-box");
  const head = el("div", "help-head");
  const title = el("h2");
  bracketTitle(title, "how to read this map");
  head.appendChild(title);
  const close = button("close", "×");
  close.setAttribute("aria-label", "Close help");
  close.addEventListener("click", handlers.onHelpClose);
  head.appendChild(close);
  box.appendChild(head);

  const grid = el("div", "help-grid");
  for (const section of HELP_SECTIONS) {
    const col = el("div", "help-col");
    col.appendChild(el("h3", undefined, section.title));
    const dl = el("dl");
    for (const [term, desc] of section.rows) {
      dl.appendChild(el("dt", undefined, term));
      dl.appendChild(el("dd", undefined, desc));
    }
    col.appendChild(dl);
    grid.appendChild(col);
  }
  box.appendChild(grid);

  const foot = el("div", "help-foot");
  foot.appendChild(
    el(
      "span",
      undefined,
      "every number on this map is a deterministic fact from fallow's static analysis; verify any finding with the fallow command shown in its panel",
    ),
  );
  box.appendChild(foot);

  overlay.appendChild(box);
  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) handlers.onHelpClose();
  });
  return overlay;
};

// ── Tour bar ────────────────────────────────────────────────────

export const buildTourBar = (handlers: OverlayHandlers): HTMLElement => {
  const bar = el("div");
  bar.id = "tour-bar";
  bar.setAttribute("role", "status");
  bar.setAttribute("aria-live", "polite");

  const step = el("span", "tour-step");
  bar.appendChild(step);
  const text = el("span", "tour-text");
  bar.appendChild(text);
  const prev = button("tour-btn", "◂");
  prev.setAttribute("aria-label", "Previous stop");
  prev.addEventListener("click", () => handlers.onTourStep(-1));
  bar.appendChild(prev);
  const next = button("tour-btn", "▸");
  next.setAttribute("aria-label", "Next stop");
  next.addEventListener("click", () => handlers.onTourStep(1));
  bar.appendChild(next);
  const end = button("tour-btn tour-end", "×");
  end.setAttribute("aria-label", "End tour");
  end.addEventListener("click", handlers.onTourEnd);
  bar.appendChild(end);
  return bar;
};

export const updateTourBar = (
  bar: HTMLElement,
  insights: Insight[],
  index: number | null,
): void => {
  bar.classList.toggle("open", index !== null);
  if (index === null) return;
  const insight = insights[index];
  const step = bar.querySelector(".tour-step");
  const text = bar.querySelector(".tour-text");
  if (step) step.textContent = `${index + 1}/${insights.length}`;
  if (text) {
    text.textContent = insight.detail ? `${insight.text} · ${insight.detail}` : insight.text;
  }
};
