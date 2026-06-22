import { useEffect, useState } from "react";
import { FileX, Loader2, MessageSquarePlus, TriangleAlert } from "lucide-react";
import {
  parseUnifiedDiff,
  parseMultiFileDiff,
  diffStats,
  type DiffRow,
  type FileDiffSection,
} from "../lib/diff";
import type { FeedTarget } from "../../../model/agent";
import { tokenize, type TokenType } from "../lib/highlight";
import { errorMessage } from "../lib/errors";
import { cn } from "@/lib/utils";
import { NoteComposer } from "./NoteComposer";

type Props = {
  file: string | null;
  base: string;
  onComment: (target: FeedTarget, note: string) => void;
};

const gutter =
  "w-12 shrink-0 select-none px-2 text-right text-[11px] tabular-nums text-muted-foreground/60";

const TOKEN_CLASS: Record<TokenType, string> = {
  keyword: "text-chart-5",
  string: "text-fallow-green",
  number: "text-fallow-amber",
  comment: "text-muted-foreground/70 italic",
  plain: "",
};

const Code = ({ text }: { text: string }) => (
  <code className="flex-1 whitespace-pre pr-3 text-foreground">
    {tokenize(text).map((t, k) => (
      <span key={k} className={TOKEN_CLASS[t.type]}>
        {t.value}
      </span>
    ))}
  </code>
);

/** Stable per-row key for the single-line composer: the new-side line when the
 * row exists in the new file, else the old-side line (a deleted line). New and
 * old line numbers are each unique within a file, and the `n`/`o` prefix keeps
 * the two namespaces from colliding. `null` only for an unanchorable row. */
const rowCommentKey = (row: DiffRow): string | null =>
  row.newNo !== null ? `n${row.newNo}` : row.oldNo !== null ? `o${row.oldNo}` : null;

const Row = ({
  row,
  showOld,
  selected,
  onSelectLine,
  onCommentLine,
}: {
  row: DiffRow;
  showOld: boolean;
  /** This row falls inside the active range selection (highlighted). */
  selected: boolean;
  /** Click/shift-click the new-line gutter to set or extend the range anchor. */
  onSelectLine: (newNo: number, shift: boolean) => void;
  /** Open a single-line composer under this row, keyed by `rowCommentKey`. */
  onCommentLine: (key: string) => void;
}) => {
  // Any line with a new OR old number is commentable: added/context anchor to the
  // new-file line, a deleted line anchors to its old-file line.
  const commentKey = rowCommentKey(row);
  const commentable = commentKey !== null;
  // Range selection is new-side only (the gutter shows the new-file line number);
  // a deleted line has no new number and must NOT be a range-select target.
  const hasNew = row.newNo !== null;
  return (
    <div
      className={cn(
        "group/row flex border-l-2 border-transparent hover:bg-muted/30",
        row.kind === "add" && "border-fallow-green/70 bg-fallow-green/10",
        row.kind === "del" && "border-fallow-red/70 bg-fallow-red/10",
        selected && "bg-primary/10",
      )}
    >
      {showOld && <span className={gutter}>{row.oldNo ?? ""}</span>}
      {hasNew ? (
        <button
          type="button"
          title="click to set range start · shift-click to extend"
          onClick={(e) => onSelectLine(row.newNo as number, e.shiftKey)}
          className={cn(gutter, "cursor-pointer hover:text-foreground hover:underline")}
        >
          {row.newNo}
        </button>
      ) : (
        <span className={gutter}>{row.newNo ?? ""}</span>
      )}
      <span
        className={cn(
          "w-4 shrink-0 select-none text-center",
          row.kind === "add" && "text-fallow-green",
          row.kind === "del" && "text-fallow-red",
          row.kind === "context" && "text-transparent",
        )}
      >
        {row.kind === "add" ? "+" : row.kind === "del" ? "-" : " "}
      </span>
      <Code text={row.text} />
      {commentable && (
        <button
          type="button"
          aria-label={`comment on line ${row.newNo ?? row.oldNo}`}
          title="comment on this line"
          onClick={() => {
            if (commentKey) onCommentLine(commentKey);
          }}
          className="mr-2 flex shrink-0 cursor-pointer items-center gap-1 self-center rounded border border-border bg-background px-1.5 py-0.5 text-[10px] text-muted-foreground opacity-0 transition-all hover:border-primary hover:text-foreground group-hover/row:opacity-100"
        >
          <MessageSquarePlus className="size-3.5" />
          comment
        </button>
      )}
    </div>
  );
};

/** First and last new-file line numbers in a hunk (for the per-hunk composer). */
const hunkLineSpan = (hunk: { rows: DiffRow[] }): { start: number; end: number } | null => {
  const newLines = hunk.rows.map((r) => r.newNo).filter((n): n is number => n !== null);
  if (newLines.length > 0) {
    return { start: Math.min(...newLines), end: Math.max(...newLines) };
  }
  // Deletion-only hunk: no new-side lines, anchor the hunk to its old-side span.
  const oldLines = hunk.rows.map((r) => r.oldNo).filter((n): n is number => n !== null);
  if (oldLines.length === 0) return null;
  return { start: Math.min(...oldLines), end: Math.max(...oldLines) };
};

/**
 * One file's diff: a sticky path/stats header followed by its hunks. Owns the
 * local line-comment state (which new-file line range is selected, and which line
 * has an open inline composer) so the rest of the diff stays untouched. A reviewer
 * routes a note at `file:line` (single) or `file:start-end` (range) back to the
 * agent through {@link FeedTarget}.kind `file_line`, the same channel file notes use.
 */
const FileSection = ({
  section,
  onComment,
}: {
  section: FileDiffSection;
  onComment: (target: FeedTarget, note: string) => void;
}) => {
  const stats = diffStats(section.hunks);
  // New files are all-additions: drop the always-empty old-line gutter so the
  // line numbers sit at the left, like GitHub. Modified files keep both columns.
  const showOld = section.hunks.some((h) => h.rows.some((r) => r.oldNo !== null));
  // The active new-file line range being selected for a comment (start/end are
  // inclusive new-file line numbers); null = nothing selected.
  const [range, setRange] = useState<{ start: number; end: number } | null>(null);
  // The single new-file line whose inline composer is open (null = none). Distinct
  // from `range`: the per-line "+" opens a composer directly; the range selection
  // opens a separate "comment on lines X-Y" composer.
  const [lineComposer, setLineComposer] = useState<string | null>(null);

  const file = section.file;

  // Click a line-number gutter: plain click sets a fresh single-line anchor;
  // shift-click extends from the existing anchor to form start..end.
  const onSelectLine = (newNo: number, shift: boolean): void => {
    setLineComposer(null);
    setRange((prev) =>
      shift && prev
        ? { start: Math.min(prev.start, newNo), end: Math.max(prev.end, newNo) }
        : { start: newNo, end: newNo },
    );
  };

  const onCommentLine = (key: string): void => {
    setRange(null);
    setLineComposer(key);
  };

  const clearRange = (): void => setRange(null);

  // The range affordance only counts as a real range when it spans >1 line; a
  // single-line selection is better served by the per-line composer.
  const isRange = range !== null && range.end > range.start;

  const isSelected = (newNo: number | null): boolean =>
    range !== null && newNo !== null && newNo >= range.start && newNo <= range.end;

  return (
    <div>
      <div className="sticky top-0 z-10 flex items-center justify-between gap-2 border-b border-border bg-muted px-3 py-1.5">
        <span className="truncate text-foreground">{file}</span>
        <span className="ml-2 shrink-0 tabular-nums">
          <span className="text-fallow-green">+{stats.added}</span>{" "}
          <span className="text-fallow-red">-{stats.removed}</span>
        </span>
      </div>
      {section.binary || section.hunks.length === 0 ? (
        <p className="px-3 py-2 text-[11px] text-muted-foreground">
          {section.binary ? "binary file" : "no textual diff"}
        </p>
      ) : (
        section.hunks.map((hunk, i) => {
          const span = hunkLineSpan(hunk);
          return (
            <div key={i}>
              <div className="flex items-center gap-2 bg-muted/40 px-3 py-1 font-mono text-[11px] text-muted-foreground">
                <span className="shrink-0 text-fallow-blue/70 tabular-nums">
                  @@ {hunk.range} @@
                </span>
                {hunk.header && (
                  <span className="truncate text-muted-foreground/80">{hunk.header}</span>
                )}
              </div>
              {hunk.rows.map((row, j) => (
                <div key={j}>
                  <Row
                    row={row}
                    showOld={showOld}
                    selected={isSelected(row.newNo)}
                    onSelectLine={onSelectLine}
                    onCommentLine={onCommentLine}
                  />
                  {/* single-line composer, full width directly under the row,
                      opened immediately by the line's comment button. Works on a
                      deleted line too: it anchors to the old-file line. */}
                  {lineComposer !== null && rowCommentKey(row) === lineComposer && (
                    <div className="bg-muted/20 px-3 py-1.5">
                      <p className="mb-1 text-[11px] text-muted-foreground">
                        line {row.newNo ?? row.oldNo}
                        {row.newNo === null ? " (deleted)" : ""}
                      </p>
                      <NoteComposer
                        defaultOpen
                        onCancel={() => setLineComposer(null)}
                        onSave={(note) => {
                          onComment(
                            { kind: "file_line", value: `${file}:${row.newNo ?? row.oldNo}` },
                            note,
                          );
                          setLineComposer(null);
                        }}
                      />
                    </div>
                  )}
                </div>
              ))}
              {/* hunk composer: full width, below the hunk's rows */}
              {span && (
                <div className="border-t border-border/40 bg-muted/10 px-3 py-1.5">
                  <NoteComposer
                    label="comment on this hunk"
                    onSave={(note) =>
                      onComment(
                        { kind: "file_line", value: `${file}:${span.start}-${span.end}` },
                        note,
                      )
                    }
                  />
                </div>
              )}
            </div>
          );
        })
      )}
      {/* range composer: surfaces once a multi-line gutter selection exists */}
      {isRange && range && (
        <div className="flex items-center gap-2 border-t border-border bg-muted/20 px-3 py-1.5">
          <span className="shrink-0 text-[11px] text-muted-foreground">
            lines {range.start}-{range.end}
          </span>
          <span className="flex-1">
            <NoteComposer
              label={`comment on lines ${range.start}-${range.end}`}
              onSave={(note) => {
                onComment(
                  { kind: "file_line", value: `${file}:${range.start}-${range.end}` },
                  note,
                );
                clearRange();
              }}
            />
          </span>
          <button
            type="button"
            onClick={clearRange}
            className="shrink-0 text-[11px] text-muted-foreground hover:text-foreground"
          >
            clear
          </button>
        </div>
      )}
    </div>
  );
};

const Centered = ({ children }: { children: React.ReactNode }) => (
  <div className="flex flex-col items-center gap-3 px-6 py-16 text-center">{children}</div>
);

export const DiffView = ({ file, base, onComment }: Props) => {
  const [sections, setSections] = useState<FileDiffSection[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setSections(null);
    setError(null);
    const load: Promise<FileDiffSection[]> = file
      ? window.fallow
          .getDiff(base, file)
          .then((d) => [
            { file, binary: d.binary, hunks: d.binary ? [] : parseUnifiedDiff(d.patch) },
          ])
      : window.fallow.getAllDiffs(base).then((d) => parseMultiFileDiff(d.patch));
    load.then((s) => active && setSections(s)).catch((e) => active && setError(errorMessage(e)));
    return () => {
      active = false;
    };
  }, [file, base]);

  if (error) {
    return (
      <Centered>
        <div className="flex size-11 items-center justify-center rounded-full border border-fallow-red/30 bg-fallow-red/10">
          <TriangleAlert className="size-5 text-fallow-red" />
        </div>
        <div className="space-y-1">
          <p className="text-sm font-medium text-foreground">couldn't load the diff</p>
          <p className="max-w-xs break-words text-xs text-muted-foreground">{error}</p>
        </div>
      </Centered>
    );
  }

  if (!sections) {
    return (
      <div className="flex items-center justify-center gap-2 py-16 text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        <span className="text-xs">loading diff…</span>
      </div>
    );
  }

  const empty = sections.every((s) => s.hunks.length === 0 && !s.binary);
  if (empty) {
    return (
      <Centered>
        <div className="flex size-11 items-center justify-center rounded-full border border-border bg-muted/30">
          <FileX className="size-5 text-muted-foreground" />
        </div>
        <div className="space-y-1">
          <p className="text-sm font-medium text-foreground">
            {file ? "no textual diff" : "no changes to review"}
          </p>
          <p className="text-xs text-muted-foreground">
            {file ? "new, binary, or unchanged file" : "this review has no file changes"}
          </p>
        </div>
      </Centered>
    );
  }

  return (
    <div data-testid="diff-scroll" className="h-full overflow-auto font-mono text-xs">
      {sections.map((section) => (
        <FileSection key={section.file} section={section} onComment={onComment} />
      ))}
    </div>
  );
};
