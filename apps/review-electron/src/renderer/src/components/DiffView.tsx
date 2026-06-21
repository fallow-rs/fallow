import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import { parseUnifiedDiff, diffStats, type DiffHunk } from "../lib/diff";
import { cn } from "@/lib/utils";

type Props = { file: string; base: string };

const gutter =
  "w-12 shrink-0 select-none px-2 text-right text-[11px] tabular-nums text-muted-foreground/60";

export const DiffView = ({ file, base }: Props) => {
  const [hunks, setHunks] = useState<DiffHunk[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setHunks(null);
    setError(null);
    window.fallow
      .getDiff(base, file)
      .then((d) => {
        if (!active) return;
        setHunks(d.binary ? [] : parseUnifiedDiff(d.patch));
      })
      .catch((e) => active && setError(e instanceof Error ? e.message : String(e)));
    return () => {
      active = false;
    };
  }, [file, base]);

  const stats = hunks ? diffStats(hunks) : null;

  return (
    <div className="h-full overflow-auto font-mono text-xs">
      <div className="sticky top-0 z-10 flex items-center justify-between border-b border-border bg-card px-3 py-1.5">
        <span className="truncate text-muted-foreground">{file}</span>
        {stats && (
          <span className="ml-2 shrink-0 tabular-nums">
            <span className="text-fallow-green">+{stats.added}</span>{" "}
            <span className="text-fallow-red">-{stats.removed}</span>
          </span>
        )}
      </div>
      {!hunks && !error && (
        <div className="flex items-center justify-center gap-2 py-16 text-muted-foreground">
          <Loader2 className="size-4 animate-spin" />
          <span className="text-xs">loading diff…</span>
        </div>
      )}
      {error && <p className="p-3 text-destructive">{error}</p>}
      {hunks?.length === 0 && !error && (
        <p className="p-3 text-muted-foreground">
          no textual diff (new, binary, or unchanged file)
        </p>
      )}
      {hunks?.map((hunk, i) => (
        <div key={i}>
          <div className="flex items-center gap-2 bg-muted/40 px-3 py-1 text-[11px] text-muted-foreground">
            <span className="text-fallow-blue/80">@@</span>
            {hunk.header && <span className="truncate">{hunk.header}</span>}
          </div>
          {hunk.rows.map((row, j) => (
            <div
              key={j}
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
              <code className="flex-1 whitespace-pre pr-3 text-foreground">{row.text}</code>
            </div>
          ))}
        </div>
      ))}
    </div>
  );
};
