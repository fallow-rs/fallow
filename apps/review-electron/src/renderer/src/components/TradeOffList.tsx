import { Scale, ShieldCheck, ShieldAlert } from "lucide-react";
import type { Severity, TradeOff, TradeOffEnvelope } from "../../../model/tradeoff";
import type { FeedTarget } from "../../../model/agent";
import type {
  TradeOffAnchorStatus,
  TradeOffValidation,
} from "../../../main/tradeoffValidation";
import { shortAnchor } from "@/lib/anchor";
import { NoteComposer } from "./NoteComposer";

/** Tone for a severity badge: only `high` gets the amber accent; the rest stay muted. */
const severityTone = (s: Severity): string =>
  s === "high" ? "text-fallow-amber" : "text-muted-foreground";

const ANCHOR_CROSS_CUTTING = "cross-cutting";

/** Render the fallow-validation status of a trade-off's anchor. `anchored` is the
 * only graph-confirmed state; the rest stay muted (the prose is always inference). */
const AnchorStatus = ({ status }: { status: TradeOffAnchorStatus | undefined }) => {
  if (status === "anchored") {
    return (
      <span className="flex items-center gap-1 text-[11px] text-fallow-green" title="fallow confirmed this region changed">
        <ShieldCheck className="size-3" />
        anchored in fallow
      </span>
    );
  }
  if (status === "unanchored") {
    return (
      <span className="flex items-center gap-1 text-[11px] text-fallow-amber" title="the anchor is not a changed region in the current diff">
        <ShieldAlert className="size-3" />
        anchor not in diff
      </span>
    );
  }
  if (status === "not-anchorable") {
    return <span className="text-[11px] text-muted-foreground/70">model-inferred (cross-cutting)</span>;
  }
  return null;
};

/**
 * The MODEL-INFERRED trade-off surface, rendered ALONGSIDE (and visually distinct
 * from) the deterministic decision surface. The whole list is fenced as
 * non-graph-fact (border + muted ground + a distinct `Scale` glyph) so the two
 * surfaces are never confused: decisions are proved from the graph, trade-offs are
 * a model reading the diff. Every item is `deterministic: false` by construction.
 *
 * Three reachable states, never a silent null on a real run:
 *   - `tradeoffs === null`  -> the elicitation was NOT run (neutral "not run").
 *   - `abstained === true`  -> it ran and found nothing consequential (quiet state).
 *   - otherwise             -> the capped, anchor-sorted list.
 */
export const TradeOffList = ({
  tradeoffs,
  validation,
  onOpenDiff,
  onComment,
}: {
  tradeoffs: TradeOffEnvelope | null;
  validation: TradeOffValidation | null;
  onOpenDiff: (path: string) => void;
  onComment: (target: FeedTarget, note: string) => void;
}) => {
  // Not run: neutral, no list, no abstain language (it never looked).
  if (tradeoffs === null) {
    return (
      <section className="space-y-2">
        <Header />
        <p className="rounded-md border border-dashed border-border bg-muted/10 p-2 text-xs text-muted-foreground">
          trade-off elicitation not run
        </p>
      </section>
    );
  }
  // Abstained: it ran and honestly found nothing rising to a real decision.
  if (tradeoffs.abstained || tradeoffs.tradeoffs.length === 0) {
    return (
      <section className="space-y-2">
        <Header />
        <p className="rounded-md border border-dashed border-border bg-muted/10 p-2 text-xs text-muted-foreground">
          looked, found nothing consequential
        </p>
      </section>
    );
  }
  return (
    <section className="space-y-2">
      <Header count={tradeoffs.tradeoffs.length} />
      {/* Stale: fallow refused the anchors because the tree moved since elicitation. */}
      {validation?.stale && (
        <p className="flex items-center gap-1.5 rounded-md border border-fallow-amber/40 bg-fallow-amber/10 p-2 text-[11px] text-fallow-amber">
          <ShieldAlert className="size-3.5 shrink-0" />
          the tree moved since these were elicited; re-run the trade-off elicitation
        </p>
      )}
      {/* Fence the WHOLE list as model-inferred (border + muted ground), the same
          treatment FramingBlock uses, so a trade-off is never read as a graph fact. */}
      <ul className="space-y-1.5 rounded-md border border-border/60 bg-muted/10 p-1.5">
        {tradeoffs.tradeoffs.map((t) => (
          <TradeOffRow
            key={t.id}
            tradeoff={t}
            status={validation?.statusById[t.id]}
            onOpenDiff={onOpenDiff}
            onComment={onComment}
          />
        ))}
      </ul>
    </section>
  );
};

/** Section header. The `Scale` glyph differentiates this surface from decisions. */
const Header = ({ count }: { count?: number }) => (
  <h3 className="flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
    <Scale className="size-3" />
    trade-offs{count === undefined ? "" : ` (${count})`}
  </h3>
);

/** A small consequence/confidence badge; never an authority signal, just a band. */
const Badge = ({ label, value }: { label: string; value: Severity }) => (
  <span className={`text-[11px] ${severityTone(value)}`}>
    {label} {value}
  </span>
);

const TradeOffRow = ({
  tradeoff: t,
  status,
  onOpenDiff,
  onComment,
}: {
  tradeoff: TradeOff;
  status: TradeOffAnchorStatus | undefined;
  onOpenDiff: (path: string) => void;
  onComment: (target: FeedTarget, note: string) => void;
}) => {
  // The cross-cutting slot has no single changed line to deep-link to.
  const linkable = t.anchor !== ANCHOR_CROSS_CUTTING && t.anchor.length > 0;
  const [anchorFile, anchorLine] = t.anchor.split(":");
  const anchorLabel = `${shortAnchor(anchorFile ?? t.anchor)}${anchorLine ? `:${anchorLine}` : ""}`;
  return (
    <li className="rounded-md border border-border bg-muted/20 p-2 text-xs">
      <div className="flex gap-2">
        <Scale className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
        <div className="min-w-0 flex-1 space-y-1">
          {/* anchor (clickable) + lens, subtle inline header , the deep-link is
              always visible here, not hidden behind a full-height toggle button */}
          <div className="flex flex-wrap items-center gap-x-2 text-[11px] text-muted-foreground">
            {linkable ? (
              <button
                type="button"
                title={t.anchor}
                className="break-all font-mono hover:text-foreground hover:underline"
                onClick={() => onOpenDiff(anchorFile ?? t.anchor)}
              >
                {anchorLabel}
              </button>
            ) : (
              <span className="font-mono">{t.anchor}</span>
            )}
            {t.lens && <span className="opacity-70">· {t.lens}</span>}
          </div>
          {/* (1) observed , a neutral fact read from the diff */}
          {t.observed && <p className="text-muted-foreground">{t.observed}</p>}
          {/* (2) trade-off , the model's inference (gain and cost) */}
          {t.tradeoff && <p className="text-muted-foreground">{t.tradeoff}</p>}
          {/* (3) question LAST , the open call the human owns (inverse of DecisionRow) */}
          <p className="text-foreground">{t.question || t.id}</p>
          <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5">
            <Badge label="consequence" value={t.consequence} />
            <Badge label="confidence" value={t.confidence} />
            <AnchorStatus status={status} />
          </div>
          {/* a note back to the agent about THIS trade-off, identified by its
              anchor (file:line) or the literal "cross-cutting" via its own kind */}
          <div className="pt-0.5">
            <NoteComposer
              onSave={(note) => onComment({ kind: "tradeoff", value: t.anchor }, note)}
            />
          </div>
        </div>
      </div>
    </li>
  );
};
