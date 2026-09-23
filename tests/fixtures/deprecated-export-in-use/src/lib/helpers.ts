/**
 * Formats the legacy value.
 * @deprecated Use {@link newHelper} instead.
 *   It goes away in the next major.
 * @see newHelper
 */
export function oldHelper(): number {
  return 1;
}

export function newHelper(): number {
  return 2;
}

/** @deprecated only an unreachable file uses it */
export const onlyDeadUse = 3;
