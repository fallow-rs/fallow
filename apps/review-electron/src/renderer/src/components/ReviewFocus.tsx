import { GitCommitHorizontal, MessageSquarePlus } from "lucide-react";
import type { ReviewFocus as Focus } from "../../../model/walkthrough";
import { cn } from "@/lib/utils";

const verdictTone = (v: string): string =>
  v === "fail"
    ? "border-fallow-red/30 bg-fallow-red/10 text-fallow-red"
    : v === "pass"
      ? "border-fallow-green/30 bg-fallow-green/10 text-fallow-green"
      : "border-border bg-muted text-muted-foreground";

const riskTone = (r: string): string =>
  r === "high" ? "text-fallow-red" : r === "medium" ? "text-fallow-amber" : "text-fallow-green";

export const ReviewFocus = ({ focus, noteCount }: { focus: Focus; noteCount: number }) => (
  <section data-testid="review-loaded" className="space-y-2.5">
    <div className="flex items-center gap-2">
      <span
        className={cn(
          "rounded-full border px-2 py-0.5 text-[11px] font-medium lowercase",
          verdictTone(focus.verdict),
        )}
      >
        {focus.verdict}
      </span>
      <span className="flex items-center gap-1 font-mono text-[11px] text-muted-foreground">
        <GitCommitHorizontal className="size-3.5" />
        {focus.baseRef.slice(0, 9)}
      </span>
    </div>
    <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1 text-sm">
      <span className="text-muted-foreground">
        <span className="font-mono tabular-nums text-foreground">{focus.changedFiles}</span> files
      </span>
      <span className="text-muted-foreground">
        risk <span className={cn("font-medium", riskTone(focus.riskClass))}>{focus.riskClass}</span>
      </span>
      <span className="text-muted-foreground">effort {focus.reviewEffort.replace(/_/g, " ")}</span>
    </div>
    {noteCount > 0 && (
      <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <MessageSquarePlus className="size-3.5" />
        <span className="font-mono tabular-nums">{noteCount}</span> note(s) sent to the agent
      </p>
    )}
  </section>
);
