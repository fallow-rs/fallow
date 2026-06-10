// Positive: sanitized output mixed with unsanitized dynamic HTML remains a candidate.
const escapeHtml = (value: string): string =>
  value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

const renderToken = (token: { content: string }): string =>
  `<span>${escapeHtml(token.content)}${token.content}</span>`;

export function renderTrustedLocalMixed(
  el: HTMLElement,
  token: { content: string },
): void {
  el.innerHTML = renderToken(token);
}
