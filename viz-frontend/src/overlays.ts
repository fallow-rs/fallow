import { button, el } from "./dom";
/**
 * HTML layers over the canvas. Currently the help overlay (how to read
 * the map); styled by the shared design tokens.
 */

export interface OverlayHandlers {
  onHelpClose: () => void;
}



// ── Help overlay ────────────────────────────────────────────────

const HELP_SECTIONS: Array<{ title: string; rows: Array<[string, string]> }> = [
  {
    title: "lenses",
    rows: [
      ["overview", "just the structure: folders and their imports, no findings"],
      ["unused", "red = file nothing imports · amber = file with unused exports"],
      ["duplication", "amber intensity = share of copy-pasted lines in the file"],
      ["boundaries", "color = architecture zone · red road = forbidden import"],
      ["complexity", "amber to red = hardest files to change (cyclomatic heat)"],
    ],
  },
  {
    title: "reading the picture",
    rows: [
      ["dot", "one file, sized by bytes; shapes group files by folder"],
      ["road", "bundled imports between folders; thick end = the importer"],
      ["left to right", "entry code on the left, shared foundations right"],
      ["×N ring", "hub file, imported by N files"],
      ["standalone", "chip bottom-left lists files no project code imports"],
      ["treemap view", "same files as nested rectangles, drill into folders"],
    ],
  },
  {
    title: "interactions",
    rows: [
      ["click a dot", "facts panel with who imports it and what it imports"],
      ["click a road", "list every file pair on that road"],
      ["shift-click ×2", "trace the shortest dependency path between two files"],
      ["/ then enter", "search, zoom to the best match"],
      ["1 to 5", "switch lens · g graph · t treemap · 0 reset view"],
      ["png", "export this view for a slide or doc"],
      ["esc", "back out of anything"],
    ],
  },
];

export const buildHelpOverlay = (handlers: OverlayHandlers): HTMLElement => {
  const overlay = el("div");
  overlay.id = "help-overlay";
  overlay.setAttribute("role", "dialog");
  overlay.setAttribute("aria-modal", "true");
  overlay.setAttribute("aria-label", "How to read this map");

  const box = el("div", "help-box");
  const head = el("div", "help-head");
  head.appendChild(el("h2", undefined, "how to read this map"));
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
  // Minimal modal focus trap: Tab cycles between the dialog's
  // focusable elements instead of escaping into the page behind it.
  overlay.addEventListener("keydown", (e) => {
    if (e.key !== "Tab") return;
    const focusables = [
      ...overlay.querySelectorAll<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      ),
    ];
    if (focusables.length === 0) return;
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  });
  return overlay;
};
