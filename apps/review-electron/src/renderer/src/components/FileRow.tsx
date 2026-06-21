import { useState } from "react";
import { FileText, Plus } from "lucide-react";
import type { WalkthroughFile } from "../../../model/walkthrough";
import { deriveFileBadges, type Tone } from "../lib/badges";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

type Props = {
  file: WalkthroughFile;
  viewed: boolean;
  onToggleViewed: (path: string) => void;
  onAddNote: (path: string, note: string) => void;
  onOpenDiff: (path: string) => void;
};

const badgeVariant = (tone: Tone): "default" | "secondary" | "outline" =>
  tone === "high" ? "default" : tone === "info" ? "secondary" : "outline";

// Status badges become row styling (dimming / accent), not repeated chips.
const STATUS_LABELS = new Set(["REVIEW HERE", "DEPRIORITIZED"]);

export const FileRow = ({ file, viewed, onToggleViewed, onAddNote, onOpenDiff }: Props) => {
  const [adding, setAdding] = useState(false);
  const [note, setNote] = useState("");

  const save = (): void => {
    if (note.trim()) onAddNote(file.path, note.trim());
    setNote("");
    setAdding(false);
  };

  const base = file.path.split("/").pop() ?? file.path;
  const dir = file.path.slice(0, file.path.length - base.length);
  const badges = deriveFileBadges(file).filter((b) => !STATUS_LABELS.has(b.label));

  return (
    <li className={cn("group rounded-md px-2 py-1.5 hover:bg-accent/40", viewed && "opacity-40")}>
      <div className="flex items-center gap-2">
        <Checkbox
          checked={viewed}
          onCheckedChange={() => onToggleViewed(file.path)}
          aria-label={`mark ${base} reviewed`}
        />
        <button
          type="button"
          data-testid="file-open"
          onClick={() => onOpenDiff(file.path)}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          <FileText className="size-3.5 shrink-0 text-muted-foreground" />
          <span className="min-w-0 flex-1 truncate font-mono text-xs">
            <span className="text-muted-foreground">{dir}</span>
            <span className="text-foreground">{base}</span>
          </span>
        </button>
        {badges.map((b) => (
          <Badge
            key={b.label}
            variant={badgeVariant(b.tone)}
            className="shrink-0 px-1.5 py-0 font-mono text-[9px] lowercase"
          >
            {b.label}
          </Badge>
        ))}
        <Button
          variant="ghost"
          size="icon"
          className="size-6 shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100"
          aria-label="add note"
          onClick={() => setAdding((a) => !a)}
        >
          <Plus className="size-3.5" />
        </Button>
      </div>
      {file.reason && (
        <p className="ml-7 truncate text-[11px] text-muted-foreground">{file.reason}</p>
      )}
      {adding && (
        <div className="ml-7 mt-1.5 flex gap-1">
          <Input
            value={note}
            onChange={(e) => setNote(e.target.value)}
            placeholder="note for the agent"
            className="h-7 text-xs"
          />
          <Button size="sm" className="h-7 text-xs lowercase" onClick={save}>
            save
          </Button>
        </div>
      )}
    </li>
  );
};
