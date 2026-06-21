import { useEffect, useState } from "react";
import { parseUnifiedDiff, diffStats, type DiffHunk } from "../lib/diff";
import { cn } from "@/lib/utils";

type Props = { file: string; base: string };

const gutter = "w-10 shrink-0 select-none px-2 text-right text-muted-foreground tabular-nums";

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
      {error && <p className="p-3 text-destructive">{error}</p>}
      {hunks?.length === 0 && !error && (
        <p className="p-3 text-muted-foreground">
          no textual diff (new, binary, or unchanged file)
        </p>
      )}
      {hunks?.map((hunk, i) => (
        <div key={i}>
          <div className="bg-muted/40 px-3 py-1 text-muted-foreground">@@ {hunk.header}</div>
          {hunk.rows.map((row, j) => (
            <div
              key={j}
              className={cn(
                "flex",
                row.kind === "add" && "bg-fallow-green/10",
                row.kind === "del" && "bg-fallow-red/10",
              )}
            >
              <span className={gutter}>{row.oldNo ?? ""}</span>
              <span className={gutter}>{row.newNo ?? ""}</span>
              <span
                className={cn(
                  "flex-1 whitespace-pre px-2",
                  row.kind === "add" && "text-fallow-green",
                  row.kind === "del" && "text-fallow-red",
                )}
              >
                {row.kind === "add" ? "+" : row.kind === "del" ? "-" : " "}
                {row.text}
              </span>
            </div>
          ))}
        </div>
      ))}
    </div>
  );
};
