/**
 * Tiny DOM builders shared by every chrome surface (toolbar, panel,
 * overlays). Pure element construction; no styling knowledge.
 */

export const el = (tag: string, cls?: string, text?: string): HTMLElement => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
};

export const button = (cls: string, text: string): HTMLButtonElement => {
  const buttonEl = document.createElement("button");
  buttonEl.type = "button";
  buttonEl.className = cls;
  buttonEl.textContent = text;
  return buttonEl;
};

let liveRegion: HTMLElement | null = null;

/**
 * Announce a transient message to assistive tech through one shared
 * polite live region. Canvas actions and clipboard copies are conveyed
 * visually only, so this is the sole screen-reader channel for them.
 */
const announce = (message: string): void => {
  if (!liveRegion) {
    liveRegion = el("div", "sr-only");
    liveRegion.setAttribute("role", "status");
    liveRegion.setAttribute("aria-live", "polite");
    document.body.appendChild(liveRegion);
  }
  // Clear first so an identical repeat message still re-announces.
  liveRegion.textContent = "";
  liveRegion.textContent = message;
};

/**
 * A copy-to-clipboard button that confirms inline and restores its
 * label; the text to copy is resolved at click time.
 */
export const copyButton = (
  cls: string,
  label: string,
  getText: () => string,
): HTMLButtonElement => {
  const buttonEl = button(cls, label);
  buttonEl.addEventListener("click", () => {
    void (async () => {
      if (!navigator.clipboard) return;
      await navigator.clipboard.writeText(getText());
      buttonEl.textContent = "Copied";
      announce("Copied to clipboard");
      setTimeout(() => {
        buttonEl.textContent = label;
      }, 1200);
    })();
  });
  return buttonEl;
};

/** The panel's dismiss button, aria-labelled and wired to a handler. */
export const closeButton = (onClose: () => void): HTMLButtonElement => {
  const buttonEl = button("icon-btn close", "×");
  buttonEl.setAttribute("aria-label", "Close details");
  buttonEl.addEventListener("click", onClose);
  return buttonEl;
};

const SVG_NS = "http://www.w3.org/2000/svg";

const svgIcon = (paths: readonly { tag: string; attrs: Record<string, string> }[]): SVGElement => {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("width", "13");
  svg.setAttribute("height", "13");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.3");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("focusable", "false");
  for (const { tag, attrs } of paths) {
    const node = document.createElementNS(SVG_NS, tag);
    for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
    svg.appendChild(node);
  }
  return svg;
};

const copyIcon = (): SVGElement =>
  svgIcon([
    { tag: "rect", attrs: { x: "5.5", y: "5.5", width: "9", height: "9", rx: "1.5" } },
    { tag: "path", attrs: { d: "M10.5 3.2V3A1.5 1.5 0 0 0 9 1.5H3A1.5 1.5 0 0 0 1.5 3v6A1.5 1.5 0 0 0 3 10.5h.2" } },
  ]);

const checkIcon = (): SVGElement => svgIcon([{ tag: "path", attrs: { d: "M3 8.5 6.5 12 13 4.5" } }]);

/**
 * An icon copy-to-clipboard button, styled like the close button. Swaps to a
 * checkmark on success and restores the copy glyph after a beat; the text to
 * copy is resolved at click time.
 */
export const copyIconButton = (
  cls: string,
  ariaLabel: string,
  getText: () => string,
): HTMLButtonElement => {
  const buttonEl = button(`icon-btn ${cls}`, "");
  buttonEl.setAttribute("aria-label", ariaLabel);
  buttonEl.title = ariaLabel;
  buttonEl.appendChild(copyIcon());
  buttonEl.addEventListener("click", () => {
    void (async () => {
      if (!navigator.clipboard) return;
      await navigator.clipboard.writeText(getText());
      buttonEl.replaceChildren(checkIcon());
      announce("Copied to clipboard");
      setTimeout(() => {
        buttonEl.replaceChildren(copyIcon());
      }, 1200);
    })();
  });
  return buttonEl;
};
