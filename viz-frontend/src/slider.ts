/**
 * Sliding selection indicator for a group of pressable buttons. The
 * indicator glides from the old choice to the new one, so a lens or view
 * switch shows where the selection went instead of blinking. The group
 * keeps owning `aria-pressed`; this module only watches it.
 */

const SELECTED = '[aria-pressed="true"]';

/**
 * Install the indicator in `group`. The first placement is instant; later
 * moves transition through CSS (`.slide-indicator` in styles.css).
 */
export const installSlider = (group: HTMLElement): void => {
  const indicator = document.createElement("span");
  indicator.className = "slide-indicator";
  indicator.setAttribute("aria-hidden", "true");
  group.prepend(indicator);
  group.classList.add("has-slider");

  const place = (): void => {
    const active = group.querySelector<HTMLElement>(SELECTED);
    // A selection that is hidden (an overflowed lens) or not laid out
    // (a collapsed group) has nothing to point at.
    if (!active || active.offsetWidth === 0) {
      indicator.style.opacity = "0";
      return;
    }
    indicator.style.opacity = "1";
    indicator.style.width = `${active.offsetWidth}px`;
    indicator.style.transform = `translateX(${active.offsetLeft}px)`;
  };

  place();
  // Enable the glide only after the first placement has painted.
  requestAnimationFrame(() => group.classList.add("slider-ready"));

  new MutationObserver(place).observe(group, {
    subtree: true,
    childList: true,
    characterData: true,
    attributes: true,
    attributeFilter: ["aria-pressed", "class"],
  });
  if (typeof ResizeObserver !== "undefined") new ResizeObserver(place).observe(group);
};
