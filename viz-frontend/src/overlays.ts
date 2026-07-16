/**
 * HTML layers over the canvas. Currently the help overlay (how to read
 * the map); styled by the shared design tokens.
 */

export interface OverlayHandlers {
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
      ["minimap", "bottom right: click to pan the camera"],
      ["1 2 3 4", "switch lens · m map · g graph · 0 reset view"],
      ["png", "export this view for a slide or doc"],
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
