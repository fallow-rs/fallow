// Negative: a direct local HTML escape helper may feed an HTML text sink.
const escapeHtml = (value: string): string =>
  value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

export function renderTrustedLocalDirect(
  el: HTMLElement,
  userInput: string,
): void {
  el.innerHTML = escapeHtml(userInput);
}
