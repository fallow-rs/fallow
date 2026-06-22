/**
 * Compact inline label for a `path[:line]` anchor: the basename (plus line), so a
 * long repo path does not overflow the narrow sidebar. Callers keep the full path
 * for the click target and the hover `title`.
 */
export const shortAnchor = (anchor: string): string => {
  const slash = anchor.lastIndexOf("/");
  return slash >= 0 ? anchor.slice(slash + 1) : anchor;
};
