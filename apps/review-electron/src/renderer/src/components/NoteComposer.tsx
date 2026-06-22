import { useState, type KeyboardEvent } from "react";
import { MessageSquarePlus } from "lucide-react";
import { Button } from "@/components/ui/button";

type Props = {
  onSave: (note: string) => void;
  /** The collapsed trigger text (e.g. "note for the agent", "comment on this hunk"). */
  label?: string;
  /** Start expanded (for surfaces opened by an explicit action, e.g. a line's
   * comment button), so the reviewer types straight away with no second click. */
  defaultOpen?: boolean;
  /** Called when the composer is cancelled, so a caller that conjured it on demand
   * (a per-line composer) can remove it rather than collapse to a trigger. */
  onCancel?: () => void;
};

/**
 * The shared "note for the agent" affordance: a subtle collapsed trigger that
 * expands to a roomy multi-line composer + a `send` button. Reused under every
 * comment surface (decisions, trade-offs, diff lines/ranges/hunks) so a reviewer
 * can route a targeted note back to the agent from anywhere the target is
 * anchored. The CALLER renders any scope label ("line 42", "lines 37-42") next to
 * this; the placeholder is always the plain "note for the agent".
 *
 * Cmd/Ctrl+Enter sends, Escape cancels; sending trims, emits, clears, and
 * collapses. Empty/whitespace never sends. The caller owns the
 * {@link import("../../../model/agent").FeedTarget}; this only collects the text.
 */
export const NoteComposer = ({
  onSave,
  label = "note for the agent",
  defaultOpen = false,
  onCancel,
}: Props) => {
  const [adding, setAdding] = useState(defaultOpen);
  const [note, setNote] = useState("");

  const send = (): void => {
    const trimmed = note.trim();
    if (trimmed) onSave(trimmed);
    setNote("");
    setAdding(false);
  };

  const cancel = (): void => {
    setNote("");
    setAdding(false);
    onCancel?.();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>): void => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      send();
    } else if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    }
  };

  if (!adding) {
    return (
      <button
        type="button"
        onClick={() => setAdding(true)}
        className="inline-flex cursor-pointer items-center gap-1 text-[11px] text-muted-foreground transition-colors hover:text-foreground"
      >
        <MessageSquarePlus className="size-3" />
        {label}
      </button>
    );
  }

  return (
    <div className="w-full space-y-1.5 rounded-md border border-border bg-background/60 p-2">
      <textarea
        autoFocus
        rows={3}
        value={note}
        onChange={(e) => setNote(e.target.value)}
        onKeyDown={onKeyDown}
        placeholder="note for the agent"
        className="w-full resize-y rounded-md border border-input bg-background px-2 py-1.5 text-xs leading-relaxed outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring/60"
      />
      <div className="flex items-center justify-between">
        <span className="text-[10px] text-muted-foreground/70">⌘↵ to send</span>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={cancel}
            className="cursor-pointer text-[11px] text-muted-foreground transition-colors hover:text-foreground"
          >
            cancel
          </button>
          <Button size="sm" className="h-7 text-xs lowercase" onClick={send}>
            send
          </Button>
        </div>
      </div>
    </div>
  );
};
