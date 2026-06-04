/**
 * Clamp a `fallow.duplication.minOccurrences` setting value to what the CLI
 * accepts. The CLI (and config deserializer) reject values below 2, so a
 * hand-edited `settings.json` that bypasses the schema `minimum` must degrade
 * to the floor rather than fail the entire sidebar run with a non-zero exit.
 * Non-integers are truncated; non-finite values fall back to the default.
 */
export const MIN_OCCURRENCES_FLOOR = 2;
export const MIN_LINES_DEFAULT = 5;
export const MIN_LINES_FLOOR = 1;

export const clampMinOccurrences = (value: number): number => {
  if (!Number.isFinite(value)) {
    return MIN_OCCURRENCES_FLOOR;
  }
  return Math.max(MIN_OCCURRENCES_FLOOR, Math.trunc(value));
};

export const clampMinLines = (value: number): number => {
  if (!Number.isFinite(value)) {
    return MIN_LINES_DEFAULT;
  }
  return Math.max(MIN_LINES_FLOOR, Math.trunc(value));
};
