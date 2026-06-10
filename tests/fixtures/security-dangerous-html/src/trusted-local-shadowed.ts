// Positive: a shadowing helper parameter must not inherit module helper trust.
const escapeHtml = (value: string): string =>
  value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

export function renderTrustedLocalShadowed(
  el: HTMLElement,
  userInput: string,
  escapeHtml: (value: string) => string,
): void {
  el.innerHTML = escapeHtml(userInput);
}
