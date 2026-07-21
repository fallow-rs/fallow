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
    title: "Lenses",
    rows: [
      ["Overview", "Just the structure: folders and their imports, no findings"],
      ["Unused", "Red for a file nothing imports, amber for one with unused exports"],
      ["Duplication", "Deeper amber means more copy-pasted lines in the file"],
      ["Boundaries", "Color is the folder layer, a red road is a forbidden import"],
      ["Complexity", "Amber through red marks the hardest files to change"],
    ],
  },
  {
    title: "Reading the picture",
    rows: [
      ["Dot", "One file, sized by bytes; shapes group files by folder"],
      ["Road", "Bundled imports between folders; the thick end is the importer"],
      ["Left to right", "Entry code on the left, shared foundations right"],
      ["×N ring", "A hub file, imported by N files"],
      ["Standalone", "Chip bottom-left lists files no project code imports"],
      ["Treemap view", "Same files as nested rectangles, drill into folders"],
      ["Zoom", "More file labels appear the further you zoom in"],
    ],
  },
  {
    title: "Interactions",
    rows: [
      ["Click a dot", "Facts panel with who imports it and what it imports"],
      ["Click a road", "List every import between those two folders"],
      ["Shift-click ×2", "Trace the shortest dependency path between two files"],
      ["/ then enter", "Search, zoom to the best match"],
      ["1 to 5", "Switch lens"],
      ["g or t", "Graph view or treemap view"],
      ["0", "Reset the view"],
      ["esc", "Back out of anything"],
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
  head.appendChild(el("h2", undefined, "How to read this map"));
  const close = button("icon-btn close", "×");
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
      "Every number on this map is a deterministic fact from fallow's static analysis; verify any finding with the fallow command shown in its panel",
    ),
  );
  box.appendChild(foot);

  overlay.appendChild(box);
  overlay.addEventListener("click", (event) => {
    if (event.target === overlay) handlers.onHelpClose();
  });
  // Minimal modal focus trap: Tab cycles between the dialog's
  // focusable elements instead of escaping into the page behind it.
  overlay.addEventListener("keydown", (event) => {
    if (event.key !== "Tab") return;
    const focusables = [
      ...overlay.querySelectorAll<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      ),
    ];
    if (focusables.length === 0) return;
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  });
  return overlay;
};
