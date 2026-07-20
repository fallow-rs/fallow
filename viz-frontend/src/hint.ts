/**
 * Floating hint tips for `[data-tip]` triggers (table-header column glosses,
 * facts-panel term definitions).
 *
 * The tip renders in ONE `<div class="hint-tip">` appended to document.body,
 * outside the detail panel's overflow context. #panel has `overflow-y: auto`,
 * which forces computed `overflow-x` to `auto` too, so a tip drawn inside the
 * panel (as an `::after` pseudo-element) gets clipped at the panel edge. A
 * body-level element escapes that clip, and `computeTipPosition` flips it
 * above / shifts it left so it always stays in the viewport.
 *
 * Events are delegated on `document`, so the single tip survives the panel
 * rebuilding its DOM (`replaceChildren`) on every render.
 */

/** Viewport margin the tip is kept inside, in px. */
const VIEWPORT_MARGIN = 8;
/** Gap between the trigger and the tip, in px. */
const TIP_GAP = 6;

/**
 * Where to place the tip given the trigger box, the tip's measured size, the
 * viewport size, and the trigger-to-tip gap. Pure: no DOM reads, unit-tested.
 *
 * Default is below the trigger, left-aligned to it. If the tip would overflow
 * the right edge it shifts left to fit; if it would overflow the bottom edge
 * it flips above the trigger instead. Left and top are finally clamped to an
 * 8px viewport margin.
 */
export const computeTipPosition = (
  trigger: DOMRect,
  tip: { width: number; height: number },
  viewport: { width: number; height: number },
  gap: number,
): { left: number; top: number } => {
  let left = trigger.left;
  let top = trigger.bottom + gap;

  if (left + tip.width > viewport.width - VIEWPORT_MARGIN) {
    left = viewport.width - VIEWPORT_MARGIN - tip.width;
  }
  if (top + tip.height > viewport.height - VIEWPORT_MARGIN) {
    top = trigger.top - gap - tip.height;
  }

  if (left < VIEWPORT_MARGIN) left = VIEWPORT_MARGIN;
  if (top < VIEWPORT_MARGIN) top = VIEWPORT_MARGIN;

  return { left, top };
};

/**
 * Install the global hint-tip delegation. Call once, after the panel is
 * mounted. Idempotent enough for a single boot; not designed to be undone.
 */
export const installHintTips = (): void => {
  const tip = document.createElement("div");
  tip.className = "hint-tip";
  tip.hidden = true;
  document.body.appendChild(tip);

  const hide = (): void => {
    if (tip.hidden) return;
    tip.hidden = true;
    tip.textContent = "";
  };

  const show = (trigger: HTMLElement): void => {
    const text = trigger.dataset.tip;
    if (!text) return;
    // Fill and unhide before measuring, so getBoundingClientRect reflects the
    // real rendered size; left/top are set synchronously after, so the tip
    // never paints at a stale position.
    tip.textContent = text;
    tip.hidden = false;
    const triggerRect = trigger.getBoundingClientRect();
    const tipRect = tip.getBoundingClientRect();
    const { left, top } = computeTipPosition(
      triggerRect,
      { width: tipRect.width, height: tipRect.height },
      { width: window.innerWidth, height: window.innerHeight },
      TIP_GAP,
    );
    tip.style.left = `${left}px`;
    tip.style.top = `${top}px`;
  };

  const resolveTrigger = (target: EventTarget | null): HTMLElement | null => {
    if (!(target instanceof HTMLElement)) return null;
    return target.closest<HTMLElement>("[data-tip]");
  };

  // A move whose destination is still inside the same trigger (e.g. onto a
  // child element) must not hide the tip, so we would not flicker.
  const staysWithin = (trigger: HTMLElement, related: EventTarget | null): boolean =>
    related instanceof Node && trigger.contains(related);

  document.addEventListener("mouseover", (event) => {
    const trigger = resolveTrigger(event.target);
    if (trigger) show(trigger);
  });
  document.addEventListener("mouseout", (event) => {
    const trigger = resolveTrigger(event.target);
    if (trigger && !staysWithin(trigger, event.relatedTarget)) hide();
  });

  document.addEventListener("focusin", (event) => {
    const trigger = resolveTrigger(event.target);
    if (trigger) show(trigger);
  });
  document.addEventListener("focusout", (event) => {
    const trigger = resolveTrigger(event.target);
    if (trigger && !staysWithin(trigger, event.relatedTarget)) hide();
  });

  // The tip is anchored to a viewport-fixed position; any scroll (the panel
  // scrolls, hence capture) invalidates that anchor, so dismiss it.
  document.addEventListener("scroll", hide, true);
  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape") hide();
  });
};
