/**
 * Shared text helpers for trusted `MarkdownString` tooltips. Extracted from the
 * verbatim copies that previously lived in both `statusBar-utils.ts` and
 * `audit-utils.ts`; behavior is identical, the single source removes drift.
 *
 * No `vscode` import, so the escaping rules can be unit-tested in isolation.
 */

/** Collapse internal whitespace runs to single spaces and trim the ends. */
export const normalizeInlineText = (value: string): string => value.replace(/\s+/g, " ").trim();

/**
 * Escape text destined for a trusted `MarkdownString` so user-derived strings
 * (git refs, file paths) cannot break out of the surrounding markdown or inject
 * a command link. Normalizes whitespace first, then backslash-escapes the
 * markdown control characters.
 *
 * Multiline counterpart: `escapeMarkdownMultiline` (preserves whitespace, escapes angle brackets).
 */
export const escapeMarkdownText = (value: string): string =>
  normalizeInlineText(value).replace(/([\\`*_{}[\]()#+.!|>-])/g, "\\$1");

/**
 * Escape text destined for a `MarkdownString` WITHOUT collapsing whitespace,
 * for multiline/block tooltip content (health recommendations, license tiers,
 * finding names). Escapes the markdown control characters, including angle
 * brackets, that could break out of the surrounding markup or smuggle a
 * command link. Counterpart: `escapeMarkdownText` is the inline variant; it
 * collapses whitespace and does not escape `<`.
 */
export const escapeMarkdownMultiline = (raw: string): string =>
  raw.replace(/([\\`*_{}[\]()#+\-.!|<>])/g, "\\$1");
