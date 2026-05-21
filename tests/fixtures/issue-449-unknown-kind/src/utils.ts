// Partial-accept case: `unused-export` is a recognized kind so it suppresses
// `secret`; `complexity-typo` is unknown and surfaces as a stale-suppression
// finding with kind_known: false.
// fallow-ignore-next-line unused-export, complexity-typo
export const secret = 1;

export const used = 'hello';

// Single-token unknown case: nothing is suppressed; the typo surfaces as
// a stale-suppression with kind_known: false.
// fallow-ignore-next-line typo-only
export const alsoUsed = 2;

// Close-typo case: edit distance 1 from `unused-export`, so the diagnostic
// must include a "Did you mean 'unused-export'?" Levenshtein suggestion.
// fallow-ignore-next-line unsed-export
export const alsoAlsoUsed = 3;

console.log(alsoUsed, alsoAlsoUsed);
