import { readSourceFromElement } from "./source";

export type PickerOptions = { bridgeUrl: string };

/**
 * In-page element picker (injected into the app-under-review in dev). Highlights
 * the element under the cursor; on click, reads its `data-fallow-source` and
 * POSTs the selection to the Electron host's localhost bridge. Returns a stop fn.
 */
export const startInspector = ({ bridgeUrl }: PickerOptions): (() => void) => {
  const overlay = document.createElement("div");
  Object.assign(overlay.style, {
    position: "fixed",
    pointerEvents: "none",
    zIndex: "2147483647",
    border: "2px solid #e5484d",
    background: "rgba(229,72,77,0.08)",
    display: "none",
  });
  document.body.appendChild(overlay);

  const onMove = (e: MouseEvent): void => {
    if (!(e.target instanceof Element)) return;
    const r = e.target.getBoundingClientRect();
    Object.assign(overlay.style, {
      display: "block",
      left: `${r.left}px`,
      top: `${r.top}px`,
      width: `${r.width}px`,
      height: `${r.height}px`,
    });
  };

  const onClick = (e: MouseEvent): void => {
    if (!(e.target instanceof Element)) return;
    const src = readSourceFromElement(e.target);
    if (!src) return;
    e.preventDefault();
    e.stopPropagation();
    void fetch(bridgeUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ file: src.file, line: src.line, column: src.column }),
    });
  };

  document.addEventListener("mousemove", onMove, true);
  document.addEventListener("click", onClick, true);
  return () => {
    document.removeEventListener("mousemove", onMove, true);
    document.removeEventListener("click", onClick, true);
    overlay.remove();
  };
};
