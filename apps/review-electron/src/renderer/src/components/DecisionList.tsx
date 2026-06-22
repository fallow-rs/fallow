import { ChevronDown, ChevronRight, GitBranchPlus, Users } from "lucide-react";
import { useState } from "react";
import type { Decision } from "../../../model/walkthrough";

/**
 * The decision surface, rendered under taste ownership: every decision is a
 * QUESTION (never an answer), graph numbers are plain facts, and the trade-off
 * clause is a named sacrifice stated as a fact, never a recommendation. The three
 * default fields (question, honest consumer count, trade-off) are always visible;
 * everything else (category, anchor, routed expert) is behind an expand so the
 * surface stays within the reviewer's working memory.
 */
export const DecisionList = ({
  decisions,
  onOpenDiff,
}: {
  decisions: Decision[];
  onOpenDiff: (path: string) => void;
}) => {
  // A quiet empty state (NOT a silent null): the human must be able to tell
  // "the surface ran and found nothing consequential" from "it is broken".
  if (decisions.length === 0) {
    return (
      <section className="space-y-2">
        <h3 className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
          decisions
        </h3>
        <p className="rounded-md border border-dashed border-border bg-muted/10 p-2 text-xs text-muted-foreground">
          no consequential structural decisions in this change
        </p>
      </section>
    );
  }
  return (
    <section className="space-y-2">
      <h3 className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        decisions ({decisions.length})
      </h3>
      <ul className="space-y-1.5">
        {decisions.map((d) => (
          <DecisionRow key={d.signalId} decision={d} onOpenDiff={onOpenDiff} />
        ))}
      </ul>
    </section>
  );
};

const DecisionRow = ({
  decision: d,
  onOpenDiff,
}: {
  decision: Decision;
  onOpenDiff: (path: string) => void;
}) => {
  const [expanded, setExpanded] = useState(false);
  const consumerLabel =
    d.internalConsumerCount === 1
      ? "1 in-repo module already depends on this"
      : `${d.internalConsumerCount} in-repo modules already depend on this`;
  return (
    <li className="rounded-md border border-border bg-muted/20 p-2 text-xs">
      <div className="flex gap-2">
        <GitBranchPlus className="size-3.5 shrink-0 text-fallow-amber" />
        <div className="min-w-0 flex-1 space-y-1">
          {/* (1) the question , primary, always interrogative */}
          <p className="text-foreground">{d.question || d.signalId}</p>
          {/* (2) the honest blast number , a graph fact */}
          <p className="text-muted-foreground">{consumerLabel}</p>
          {/* (3) the trade-off clause , a named sacrifice stated as fact */}
          {d.tradeoff && <p className="text-muted-foreground">{d.tradeoff}</p>}
          {expanded && (
            <div className="space-y-1 border-t border-border/60 pt-1 text-[11px] text-muted-foreground">
              {d.category && <p>category: {d.category}</p>}
              {d.anchorFile && (
                <button
                  type="button"
                  className="text-left text-fallow-amber hover:underline"
                  onClick={() => onOpenDiff(d.anchorFile)}
                >
                  {d.anchorFile}
                  {d.anchorLine > 0 ? `:${d.anchorLine}` : ""}
                </button>
              )}
              {d.expert.length > 0 && (
                <p className="flex items-center gap-1">
                  <Users className="size-3" />
                  ask: {d.expert.join(", ")}
                  {d.busFactorOne ? " (sole owner)" : ""}
                </p>
              )}
            </div>
          )}
        </div>
        <button
          type="button"
          aria-label={expanded ? "collapse decision detail" : "expand decision detail"}
          className="shrink-0 text-muted-foreground hover:text-foreground"
          onClick={() => setExpanded((e) => !e)}
        >
          {expanded ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
        </button>
      </div>
    </li>
  );
};
