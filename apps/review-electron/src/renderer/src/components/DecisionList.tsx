import { GitBranchPlus, Users } from "lucide-react";
import type { Decision } from "../../../model/walkthrough";
import type { FeedTarget, FramingOrigin, InlineFraming } from "../../../model/agent";
import type { FramingBySignal } from "@/lib/agentFraming";
import { shortAnchor } from "@/lib/anchor";
import { NoteComposer } from "./NoteComposer";

/** Per-origin label + tone for a fenced inline-framing block (never a fact). */
const ORIGIN_PRESENTATION: Record<FramingOrigin, { label: string; tone: string }> = {
  // Fact-ish: recorded by the author agent at write-time. Neutral/affirmative
  // tone, but still fenced + deterministic:false: it is intent, not a graph fact.
  captured: {
    label: "captured at write-time (author agent):",
    tone: "text-fallow-green",
  },
  // Review-time inference from the opt-in agent run. Amber/muted + the explicit
  // "confirm with author" warning: a confident-wrong reconstruction is the worst
  // failure mode, so this treatment is non-negotiable.
  reconstructed: {
    label: "agent framing (unverified, confirm with author):",
    tone: "text-fallow-amber",
  },
};

/**
 * The decision surface, rendered under taste ownership: every decision is a
 * QUESTION (never an answer), graph numbers are plain facts, and the trade-off
 * clause is a named sacrifice stated as a fact, never a recommendation. The three
 * default fields (question, honest consumer count, trade-off) are always visible;
 * everything else (category, anchor, routed expert) is behind an expand so the
 * surface stays within the reviewer's working memory.
 *
 * Inline framing (captured or reconstructed), keyed by `signalId`, renders fenced
 * under its own decision so the reviewer never re-joins it from a separate panel.
 */
export const DecisionList = ({
  decisions,
  onOpenDiff,
  onComment,
  framingBySignal,
}: {
  decisions: Decision[];
  onOpenDiff: (path: string) => void;
  onComment: (target: FeedTarget, note: string) => void;
  framingBySignal?: FramingBySignal;
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
          <DecisionRow
            key={d.signalId}
            decision={d}
            onOpenDiff={onOpenDiff}
            onComment={onComment}
            framing={framingBySignal?.get(d.signalId) ?? []}
          />
        ))}
      </ul>
    </section>
  );
};

const DecisionRow = ({
  decision: d,
  onOpenDiff,
  onComment,
  framing,
}: {
  decision: Decision;
  onOpenDiff: (path: string) => void;
  onComment: (target: FeedTarget, note: string) => void;
  framing: ReadonlyArray<InlineFraming>;
}) => {
  const linkable = d.anchorFile.length > 0;
  // Only show the in-repo consumer count when fallow actually emitted it (the
  // honest count is > 0); a missing field must NOT render as a contradictory "0".
  const consumerLabel =
    d.internalConsumerCount === 1
      ? "1 in-repo module already depends on this"
      : `${d.internalConsumerCount} in-repo modules already depend on this`;
  return (
    <li className="rounded-md border border-border bg-muted/20 p-2 text-xs">
      <div className="flex gap-2">
        <GitBranchPlus className="mt-0.5 size-3.5 shrink-0 text-fallow-amber" />
        <div className="min-w-0 flex-1 space-y-1">
          {/* anchor (clickable) + category + routed expert, subtle inline header;
              the anchor is always visible here, not behind a full-height toggle */}
          <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-muted-foreground">
            {linkable && (
              <button
                type="button"
                title={d.anchorFile}
                className="break-all font-mono hover:text-foreground hover:underline"
                onClick={() => onOpenDiff(d.anchorFile)}
              >
                {shortAnchor(d.anchorFile)}
                {d.anchorLine > 0 ? `:${d.anchorLine}` : ""}
              </button>
            )}
            {d.category && <span className="opacity-70">· {d.category}</span>}
            {d.expert.length > 0 && (
              <span className="flex items-center gap-1 opacity-70">
                <Users className="size-3" />
                {d.expert.join(", ")}
                {d.busFactorOne ? " (sole owner)" : ""}
              </span>
            )}
          </div>
          {/* (1) the question , primary, always interrogative */}
          <p className="text-foreground">{d.question || d.signalId}</p>
          {/* (2) the honest blast number , a graph fact, only when emitted */}
          {d.internalConsumerCount > 0 && <p className="text-muted-foreground">{consumerLabel}</p>}
          {/* (3) the trade-off clause , a named sacrifice stated as fact */}
          {d.tradeoff && <p className="text-muted-foreground">{d.tradeoff}</p>}
          {/* (4) framing , fenced, interrogative-leaning, never a graph fact */}
          {framing.map((f, i) => (
            <FramingBlock key={`${f.origin}:${i}`} framing={f} />
          ))}
          {/* (5) a note back to the agent, anchored to THIS decision's signal */}
          <div className="pt-0.5">
            <NoteComposer
              onSave={(note) => onComment({ kind: "signal_id", value: d.signalId }, note)}
            />
          </div>
        </div>
      </div>
    </li>
  );
};

/**
 * One fenced framing block under a decision. Typographically fenced (rounded
 * border + muted ground), deterministic:false by construction, and labelled by
 * origin so the reviewer can tell author intent (captured, fact-ish) from
 * review-time inference (reconstructed, confirm-with-author). It never gates the
 * reviewer's options, never carries a door/reversibility label, never ranks.
 */
const FramingBlock = ({ framing: f }: { framing: InlineFraming }) => {
  const { label, tone } = ORIGIN_PRESENTATION[f.origin];
  return (
    <div className="space-y-0.5 rounded-md border border-border/60 bg-muted/10 p-1.5">
      <p className={tone}>{label}</p>
      <p className="text-foreground">{f.framing}</p>
      {f.concern && <p className="text-muted-foreground">concern: {f.concern}</p>}
    </div>
  );
};
