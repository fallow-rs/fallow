// Negative: local helpers that prove HTML escaping may feed HTML text sinks.
const escapeHtml = (value: string): string =>
  value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

const renderToken = (token: { content: string }): string =>
  `<span>${escapeHtml(token.content)}</span>`;

export function renderTrustedLocalEscape(
  el: HTMLElement,
  token: { content: string },
): void {
  el.innerHTML = renderToken(token);
}
