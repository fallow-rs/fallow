/** Minimal unified-diff parser (codiff-style hunk model, zero deps). */
export type DiffRowKind = "context" | "add" | "del";
export type DiffRow = {
  kind: DiffRowKind;
  oldNo: number | null;
  newNo: number | null;
  text: string;
};
export type DiffHunk = { header: string; range: string; rows: DiffRow[] };

const HUNK_RE = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/;

export const parseUnifiedDiff = (patch: string): DiffHunk[] => {
  const hunks: DiffHunk[] = [];
  let current: DiffHunk | null = null;
  let oldNo = 0;
  let newNo = 0;

  for (const line of patch.split("\n")) {
    const m = HUNK_RE.exec(line);
    if (m) {
      oldNo = Number(m[1]);
      newNo = Number(m[2]);
      // The "-a,b +c,d" portion between the @@ markers (for a real hunk header).
      const rangeEnd = line.indexOf(" @@", 3);
      const range = rangeEnd > 0 ? line.slice(3, rangeEnd) : "";
      current = { header: (m[3] ?? "").trim(), range, rows: [] };
      hunks.push(current);
      continue;
    }
    // Skip file headers (diff --git, index, ---, +++) before the first hunk.
    if (!current) continue;
    // "\ No newline at end of file" markers carry no line.
    if (line.startsWith("\\")) continue;

    const marker = line[0];
    if (marker === "+") {
      current.rows.push({ kind: "add", oldNo: null, newNo, text: line.slice(1) });
      newNo += 1;
    } else if (marker === "-") {
      current.rows.push({ kind: "del", oldNo, newNo: null, text: line.slice(1) });
      oldNo += 1;
    } else if (marker === " ") {
      current.rows.push({ kind: "context", oldNo, newNo, text: line.slice(1) });
      oldNo += 1;
      newNo += 1;
    }
  }
  return hunks;
};

/** One file's worth of diff within a multi-file `git diff` patch. */
export type FileDiffSection = { file: string; hunks: DiffHunk[]; binary: boolean };

/** Path a file section reports, preferring the new side, then the old. */
const sectionPath = (block: string): string => {
  const plus = /^\+\+\+ (?:b\/)?(.+)$/m.exec(block);
  if (plus && plus[1] !== "/dev/null") return plus[1] ?? "file";
  const minus = /^--- (?:a\/)?(.+)$/m.exec(block);
  if (minus && minus[1] !== "/dev/null") return minus[1] ?? "file";
  const git = /^diff --git a\/.+ b\/(.+)$/m.exec(block);
  return git?.[1] ?? "file";
};

/** Split a full `git diff` patch into per-file sections, each with its hunks. */
export const parseMultiFileDiff = (patch: string): FileDiffSection[] => {
  const blocks: string[][] = [];
  let current: string[] | null = null;
  for (const line of patch.split("\n")) {
    if (line.startsWith("diff --git ")) {
      current = [line];
      blocks.push(current);
    } else current?.push(line);
  }
  return blocks.map((lines) => {
    const block = lines.join("\n");
    const binary = /^Binary files /m.test(block);
    return { file: sectionPath(block), binary, hunks: binary ? [] : parseUnifiedDiff(block) };
  });
};

export const diffStats = (hunks: ReadonlyArray<DiffHunk>): { added: number; removed: number } => {
  let added = 0;
  let removed = 0;
  for (const hunk of hunks) {
    for (const row of hunk.rows) {
      if (row.kind === "add") added += 1;
      else if (row.kind === "del") removed += 1;
    }
  }
  return { added, removed };
};
