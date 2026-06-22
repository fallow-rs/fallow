import { NotebookPen } from "lucide-react";
import type { ReviewContext as ReviewContextData, ReviewContextItem } from "../../../model/reviewContext";

/**
 * The AUTHOR-AGENT review brief: the CAPTURE artifact, rendered prominently at the
 * TOP of the orientation area as the first thing the human reads. This is the agent
 * that DID the work recording, at write-time, "what the change is + the specific
 * things that need your taste".
 *
 * It is fact-ish (the author SAID it), so it is NOT fenced as hard model-inference
 * the way {@link import("./TradeOffList").TradeOffList} is; it is presented calmly as
 * the author's recorded context. Distinct from the live "Agent Review" reconstruct
 * panel, which re-derives framing after the fact rather than reading what the author
 * wrote down.
 *
 * Honesty rule: when `context === null` the author recorded no brief, so this renders
 * NOTHING (no "not run" chrome). The surface is simply absent.
 */
export const ReviewContext = ({
  context,
  onOpenDiff,
}: {
  context: ReviewContextData | null;
  onOpenDiff: (path: string) => void;
}) => {
  // No author brief: render nothing. The orientation surface is simply absent.
  if (context === null) return null;
  return (
    <section
      aria-label="from the author agent"
      className="space-y-2 rounded-md border border-border border-l-2 border-l-primary/60 bg-muted/10 p-3"
    >
      {/* Header: muted label + author attribution as a small mono chip. */}
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <NotebookPen className="size-3.5 shrink-0 text-muted-foreground" />
        <h3 className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
          from the author agent
        </h3>
        {context.author && (
          <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground">
            {context.author}
          </span>
        )}
      </div>

      {/* The overview: what the change does + where taste is needed. */}
      {context.summary && <p className="text-sm text-foreground">{context.summary}</p>}

      {/* The specific anchored asks. */}
      {context.items.length > 0 && (
        <div className="space-y-1.5">
          <h4 className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
            what to review here
          </h4>
          <ul className="space-y-1.5">
            {context.items.map((item, i) => (
              <ReviewContextRow key={`${item.anchor}:${i}`} item={item} onOpenDiff={onOpenDiff} />
            ))}
          </ul>
        </div>
      )}

      {/* Provenance footer: anchored author prose, never graph-validated content. */}
      <p className="text-[11px] text-muted-foreground">
        recorded by the author agent at write time; anchored, not graph-validated content.
      </p>
    </section>
  );
};

const ReviewContextRow = ({
  item,
  onOpenDiff,
}: {
  item: ReviewContextItem;
  onOpenDiff: (path: string) => void;
}) => {
  // An empty anchor is a general note: render the note alone, no dead link.
  const linkable = item.anchor.length > 0;
  const [anchorFile] = item.anchor.split(":");
  return (
    <li className="text-xs text-foreground">
      {linkable && (
        <button
          type="button"
          className="mr-2 font-mono text-[11px] text-muted-foreground hover:text-foreground hover:underline"
          onClick={() => onOpenDiff(anchorFile ?? item.anchor)}
        >
          {item.anchor}
        </button>
      )}
      <span>{item.note}</span>
    </li>
  );
};
