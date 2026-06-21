/** Minimal unified-diff parser (codiff-style hunk model, zero deps). */
export type DiffRowKind = "context" | "add" | "del";
export type DiffRow = {
  kind: DiffRowKind;
  oldNo: number | null;
  newNo: number | null;
  text: string;
};
export type DiffHunk = { header: string; rows: DiffRow[] };

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
      current = { header: (m[3] ?? "").trim(), rows: [] };
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
