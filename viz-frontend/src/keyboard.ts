/** True when keyboard focus sits in a text field. */
export const isTextInput = (target: EventTarget | null): boolean => {
  const tag = (target as HTMLElement | null)?.tagName;
  return tag === "INPUT" || tag === "TEXTAREA";
};

// Safari can send the IME commit keydown after compositionend.
const IME_PROCESS_KEY = 229;

/** Whether the window can use this key for dashboard navigation or search. */
export const isDashboardKey = (
  event: Pick<
    KeyboardEvent,
    "defaultPrevented" | "isComposing" | "keyCode" | "ctrlKey" | "metaKey" | "altKey"
  >,
): boolean =>
  !(
    event.defaultPrevented ||
    event.isComposing ||
    event.keyCode === IME_PROCESS_KEY ||
    event.ctrlKey ||
    event.metaKey ||
    event.altKey
  );
