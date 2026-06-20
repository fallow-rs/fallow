/** A source location stamped onto a DOM element by the Fallow inspector plugin. */
export type Source = { file: string; line: number; column: number };

/** Parse a `file:line:col` source attribute (file may itself contain colons). */
export const parseSourceAttr = (value: string): Source | null => {
  const colCut = value.lastIndexOf(":");
  const lineCut = value.lastIndexOf(":", colCut - 1);
  if (lineCut < 0 || colCut < 0) return null;
  const file = value.slice(0, lineCut);
  const line = Number(value.slice(lineCut + 1, colCut));
  const column = Number(value.slice(colCut + 1));
  if (!file || Number.isNaN(line) || Number.isNaN(column)) return null;
  return { file, line, column };
};

/** Minimal element shape the reader needs (testable without a real DOM). */
export type SourceElement = {
  closest: (selector: string) => { getAttribute: (name: string) => string | null } | null;
};

/** Walk up from `el` to the nearest element carrying `data-fallow-source`. */
export const readSourceFromElement = (el: SourceElement): Source | null => {
  const node = el.closest("[data-fallow-source]");
  const value = node?.getAttribute("data-fallow-source") ?? null;
  return value ? parseSourceAttr(value) : null;
};
