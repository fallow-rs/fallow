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
  const b = document.createElement("button");
  b.type = "button";
  b.className = cls;
  b.textContent = text;
  return b;
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
  const b = button(cls, label);
  b.addEventListener("click", () => {
    void navigator.clipboard?.writeText(getText()).then(() => {
      b.textContent = "copied";
      setTimeout(() => {
        b.textContent = label;
      }, 1200);
    });
  });
  return b;
};

/** The panel's dismiss button, aria-labelled and wired to a handler. */
export const closeButton = (onClose: () => void): HTMLButtonElement => {
  const b = button("close", "×");
  b.setAttribute("aria-label", "Close details");
  b.addEventListener("click", onClose);
  return b;
};
