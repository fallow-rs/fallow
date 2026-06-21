import type { ReactNode } from "react";
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

const Stat = ({
  label,
  children,
  className,
}: {
  label: string;
  children: ReactNode;
  className?: string;
}) => (
  <div className="min-w-0">
    <dt className="text-[10px] uppercase tracking-wider text-muted-foreground">{label}</dt>
    <dd className={cn("mt-0.5 truncate font-mono text-sm tabular-nums lowercase", className)}>
      {children}
    </dd>
  </div>
);

export const ReviewFocus = ({ focus, noteCount }: { focus: Focus; noteCount: number }) => (
  <section data-testid="review-loaded" className="space-y-3">
    <div className="flex items-center justify-between gap-2">
      <span
        className={cn(
          "rounded-full border px-2.5 py-0.5 text-xs font-semibold lowercase",
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
    <dl className="grid grid-cols-3 gap-3">
      <Stat label="files" className="text-foreground">
        {focus.changedFiles}
      </Stat>
      <Stat label="risk" className={cn("font-medium", riskTone(focus.riskClass))}>
        {focus.riskClass}
      </Stat>
      <Stat label="effort" className="text-foreground">
        {focus.reviewEffort.replace(/_/g, " ")}
      </Stat>
    </dl>
    {noteCount > 0 && (
      <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <MessageSquarePlus className="size-3.5" />
        <span className="font-mono tabular-nums">{noteCount}</span> note(s) sent to the agent
      </p>
    )}
  </section>
);
