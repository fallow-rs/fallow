import * as path from "node:path";

export interface ResolvedPath {
  readonly absolute: string;
  readonly relative: string;
}

export const resolveFilePath = (
  filePath: string | undefined,
  workspaceRoot: string | undefined,
): ResolvedPath => {
  if (!filePath) {
    return { absolute: "", relative: "" };
  }
  const absolute =
    workspaceRoot && !path.isAbsolute(filePath) ? path.resolve(workspaceRoot, filePath) : filePath;
  const relative = workspaceRoot ? path.relative(workspaceRoot, absolute) : filePath;
  return { absolute, relative };
};

/** Single-character ellipsis used for middle-truncated path display. */
export const ELLIPSIS = "…";

/**
 * Middle-truncate a relative path for display so the leading directory context
 * AND the basename both stay visible. VS Code's `TreeItem.description` exposes
 * no truncation-mode API and end-truncates by default, hiding the basename (the
 * most identifying part) on narrow panels. This keeps both ends instead.
 *
 * Path-segment-aware: keeps the first segment and the basename, collapsing the
 * interior segments to a single ellipsis, then grows the kept tail while it
 * still fits `maxLen`. Falls back to a character-level head+tail elide when even
 * `first/.../basename` exceeds the budget. Returns the input unchanged when it
 * already fits. Callers append any `:line` suffix AFTER eliding, so a line
 * number is never truncated.
 */
export const middleElidePath = (relativePath: string, maxLen = 40): string => {
  if (relativePath.length <= maxLen) {
    return relativePath;
  }
  const segments = relativePath.split("/");
  if (segments.length >= 3) {
    const first = segments[0];
    const last = segments[segments.length - 1];
    let candidate = `${first}/${ELLIPSIS}/${last}`;
    if (candidate.length <= maxLen) {
      for (let i = segments.length - 2; i > 0; i--) {
        const grown = `${first}/${ELLIPSIS}/${segments.slice(i).join("/")}`;
        if (grown.length > maxLen) {
          break;
        }
        candidate = grown;
      }
      return candidate;
    }
  }
  const keep = Math.max(1, maxLen - 1);
  const head = Math.ceil(keep / 2);
  const tail = keep - head;
  const tailPart = tail > 0 ? relativePath.slice(relativePath.length - tail) : "";
  return `${relativePath.slice(0, head)}${ELLIPSIS}${tailPart}`;
};

/**
 * Order clone groups by impact: total duplicated lines (`line_count` times the
 * number of instances) descending, then by `line_count` descending. Returns a
 * new array; the input is not mutated. Makes the `Clone #N` ordinal a real rank
 * so the largest, most-worth-extracting clone is first.
 */
export const sortCloneGroupsBySize = <
  T extends { readonly line_count: number; readonly instances: ReadonlyArray<unknown> },
>(
  groups: ReadonlyArray<T>,
): T[] =>
  [...groups].sort((a, b) => {
    const impact = b.line_count * b.instances.length - a.line_count * a.instances.length;
    return impact !== 0 ? impact : b.line_count - a.line_count;
  });
