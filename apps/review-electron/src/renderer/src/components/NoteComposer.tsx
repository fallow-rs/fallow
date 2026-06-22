import { useState, type KeyboardEvent } from "react";
import { MessageSquarePlus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

type Props = {
  onSave: (note: string) => void;
  placeholder?: string;
  label?: string;
};

/**
 * The shared "note for the agent" affordance: a subtle collapsed trigger that
 * expands to an `Input` + `save` button, mirroring FileRow's note UI. Reused under
 * every comment surface (decisions, trade-offs, diff lines) so a reviewer can
 * route a targeted note back to the agent from anywhere the target is anchored.
 *
 * `Enter` saves, `Escape` cancels; saving trims, emits, clears, and collapses.
 * Empty/whitespace never saves. The caller owns the {@link import("../../../model/agent").FeedTarget};
 * this component only collects the note text.
 */
export const NoteComposer = ({
  onSave,
  placeholder = "note for the agent",
  label = "note for the agent",
}: Props) => {
  const [adding, setAdding] = useState(false);
  const [note, setNote] = useState("");

  const save = (): void => {
    const trimmed = note.trim();
    if (trimmed) onSave(trimmed);
    setNote("");
    setAdding(false);
  };

  const cancel = (): void => {
    setNote("");
    setAdding(false);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>): void => {
    if (e.key === "Enter") {
      e.preventDefault();
      save();
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
        className="inline-flex items-center gap-1 text-[11px] text-muted-foreground transition-colors hover:text-foreground"
      >
        <MessageSquarePlus className="size-3" />
        {label}
      </button>
    );
  }

  return (
    <div className="flex gap-1">
      <Input
        autoFocus
        value={note}
        onChange={(e) => setNote(e.target.value)}
        onKeyDown={onKeyDown}
        placeholder={placeholder}
        className="h-7 text-xs"
      />
      <Button size="sm" className="h-7 text-xs lowercase" onClick={save}>
        save
      </Button>
    </div>
  );
};
