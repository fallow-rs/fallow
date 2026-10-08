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
      ["Overview", "Findings from all lenses, on folders and their imports"],
      ["Unused", "Red: no file imports this file. Ochre: the file has unused exports"],
      ["Duplication", "Deeper ochre: more duplicated lines in the file"],
      ["Architecture", "Color is the layer. Red: a forbidden import or an import cycle"],
      ["Health", "Ochre: medium risk. Red: high risk (complex code, low test coverage)"],
      ["Security", "Ochre: medium candidate. Red: high candidate. Examine each one"],
    ],
  },
  {
    title: "How to read the map",
    rows: [
      ["Dot", "One file, sized by bytes; shapes group files by folder"],
      ["Line", "Imports between two folders; the thick end is the importer"],
      ["Left to right", "Entry points on the left, shared code on the right"],
      ["×N ring", "A file that N files import"],
      ["Not connected", "Folders with no imports to or from other folders; button at bottom left"],
      ["Treemap view", "The same files as nested rectangles; click a folder to zoom in"],
      ["Zoom", "More file labels appear the further you zoom in"],
    ],
  },
  {
    title: "Interactions",
    rows: [
      ["Click a dot", "File details: findings, importers, and imports"],
      ["Click a line", "All imports between those two folders"],
      ["Shift-click ×2", "Trace the shortest dependency path between two files"],
      ["/ then enter", "Search, then zoom to the best match"],
      ["1 to 6", "Switch lens"],
      ["j or k", "Next or previous finding, with a file open"],
      ["g or t", "Graph view or treemap view"],
      ["0", "Reset the view"],
      ["esc", "Close the open panel or view"],
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
