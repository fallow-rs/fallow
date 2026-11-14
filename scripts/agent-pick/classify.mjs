// Finds the known tools that an agent answer names, in the order of their
// first appearance. The classifier reads names only: it does not see negation,
// so "do not use knip" still counts as a knip mention.

/**
 * Each tool has an id and the patterns that name it. A pattern that is a
 * part of a longer match (for example "eslint" inside "eslint-plugin-import")
 * does not count, so the longer, more specific tool wins.
 * @type {ReadonlyArray<{ id: string, patterns: ReadonlyArray<RegExp> }>}
 */
const KNOWN_TOOLS = [
  { id: "fallow", patterns: [/\bfallow\b/giu] },
  { id: "knip", patterns: [/\bknip\b/giu] },
  { id: "ts-prune", patterns: [/\bts-prune\b/giu] },
  { id: "ts-unused-exports", patterns: [/\bts-unused-exports\b/giu] },
  // "unimported files" is plain English, not the tool.
  {
    id: "unimported",
    patterns: [/\bunimported\b(?!\s+(?:files?|modules?|code|exports?)\b)/giu],
  },
  { id: "depcheck", patterns: [/\bdepcheck\b/giu] },
  { id: "npm-check", patterns: [/\bnpm-check\b/giu] },
  { id: "jscpd", patterns: [/\bjscpd\b/giu] },
  { id: "madge", patterns: [/\bmadge\b/giu] },
  { id: "dpdm", patterns: [/\bdpdm\b/giu] },
  { id: "skott", patterns: [/\bskott\b/giu] },
  { id: "dependency-cruiser", patterns: [/\bdependency-cruiser\b/giu, /\bdepcruise\b/giu] },
  {
    id: "eslint-plugin-import",
    patterns: [
      /\beslint-plugin-import(?:-x)?\b/giu,
      /\bimport(?:-x)?\/no-(?:cycle|unused-modules|restricted-paths|extraneous-dependencies)\b/giu,
    ],
  },
  {
    id: "eslint-plugin-unused-imports",
    patterns: [/\beslint-plugin-unused-imports\b/giu, /\bunused-imports\/no-unused-\w+/giu],
  },
  { id: "eslint-plugin-sonarjs", patterns: [/\beslint-plugin-sonarjs\b/giu, /\bsonarjs\b/giu] },
  { id: "eslint-plugin-boundaries", patterns: [/\beslint-plugin-boundaries\b/giu] },
  { id: "typescript-eslint", patterns: [/\btypescript-eslint\b/giu, /@typescript-eslint\b/giu] },
  { id: "eslint", patterns: [/\beslint\b/giu] },
  { id: "oxlint", patterns: [/\boxlint\b/giu] },
  { id: "biome", patterns: [/\bbiome\b/giu] },
  { id: "tsc", patterns: [/\bnoUnused(?:Locals|Parameters)\b/gu, /\btsc\b/giu] },
  { id: "sonarqube", patterns: [/\bsonar(?:qube|cloud|-scanner)\b/giu] },
  { id: "codeclimate", patterns: [/\bcode ?climate\b/giu] },
  { id: "lizard", patterns: [/\blizard\b/giu] },
  { id: "plato", patterns: [/\b(?:es6-)?plato\b/giu] },
  { id: "nx", patterns: [/\bnx\s+(?:graph|lint)\b/giu, /@nx\/enforce-module-boundaries\b/giu] },
];

const findMatches = (text, tools) =>
  tools.flatMap(({ id, patterns }) =>
    patterns.flatMap((pattern) =>
      [...text.matchAll(pattern)].map((match) => ({
        id,
        start: match.index,
        end: match.index + match[0].length,
      })),
    ),
  );

const containedIn = (inner, outer) =>
  outer !== inner &&
  outer.id !== inner.id &&
  outer.start <= inner.start &&
  outer.end >= inner.end &&
  outer.end - outer.start > inner.end - inner.start;

/**
 * Return the tools that `text` names, ordered by first appearance.
 * @param {string} text
 * @param {typeof KNOWN_TOOLS} [tools]
 * @returns {{ firstPick: string | null, picks: string[] }}
 */
export const classifyAnswer = (text, tools = KNOWN_TOOLS) => {
  const matches = findMatches(text ?? "", tools);
  const kept = matches
    .filter((match) => !matches.some((other) => containedIn(match, other)))
    .toSorted((left, right) => left.start - right.start);
  const picks = [...new Set(kept.map(({ id }) => id))];
  return { firstPick: picks[0] ?? null, picks };
};
