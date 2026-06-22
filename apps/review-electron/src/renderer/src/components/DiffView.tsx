import { useEffect, useState } from "react";
import { FileX, Loader2, TriangleAlert } from "lucide-react";
import {
  parseUnifiedDiff,
  parseMultiFileDiff,
  diffStats,
  type DiffRow,
  type FileDiffSection,
} from "../lib/diff";
import { tokenize, type TokenType } from "../lib/highlight";
import { errorMessage } from "../lib/errors";
import { cn } from "@/lib/utils";

type Props = { file: string | null; base: string };

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

const Row = ({ row }: { row: DiffRow }) => (
  <div
    className={cn(
      "flex border-l-2 border-transparent hover:bg-muted/30",
      row.kind === "add" && "border-fallow-green/70 bg-fallow-green/10",
      row.kind === "del" && "border-fallow-red/70 bg-fallow-red/10",
    )}
  >
    <span className={gutter}>{row.oldNo ?? ""}</span>
    <span className={gutter}>{row.newNo ?? ""}</span>
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
  </div>
);

/** One file's diff: a sticky path/stats header followed by its hunks. */
const FileSection = ({ section }: { section: FileDiffSection }) => {
  const stats = diffStats(section.hunks);
  return (
    <div>
      <div className="sticky top-0 z-10 flex items-center justify-between gap-2 border-b border-border bg-card px-3 py-1.5">
        <span className="truncate text-muted-foreground">{section.file}</span>
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
        section.hunks.map((hunk, i) => (
          <div key={i}>
            <div className="flex items-center gap-2 bg-muted/40 px-3 py-1 font-mono text-[11px] text-muted-foreground">
              <span className="shrink-0 text-fallow-blue/70 tabular-nums">@@ {hunk.range} @@</span>
              {hunk.header && (
                <span className="truncate text-muted-foreground/80">{hunk.header}</span>
              )}
            </div>
            {hunk.rows.map((row, j) => (
              <Row key={j} row={row} />
            ))}
          </div>
        ))
      )}
    </div>
  );
};

const Centered = ({ children }: { children: React.ReactNode }) => (
  <div className="flex flex-col items-center gap-3 px-6 py-16 text-center">{children}</div>
);

export const DiffView = ({ file, base }: Props) => {
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
    <div className="h-full overflow-auto font-mono text-xs">
      {sections.map((section) => (
        <FileSection key={section.file} section={section} />
      ))}
    </div>
  );
};
