import { useState } from "react";
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

export const FileRow = ({ file, viewed, onToggleViewed, onAddNote, onOpenDiff }: Props) => {
  const [adding, setAdding] = useState(false);
  const [note, setNote] = useState("");

  const save = (): void => {
    if (note.trim()) onAddNote(file.path, note.trim());
    setNote("");
    setAdding(false);
  };

  return (
    <li className={cn("border-b border-border py-1.5", viewed && "opacity-45")}>
      <div className="flex items-baseline gap-2">
        <Checkbox
          checked={viewed}
          onCheckedChange={() => onToggleViewed(file.path)}
          className="mt-0.5"
          aria-label={`mark ${file.path} reviewed`}
        />
        <button
          type="button"
          data-testid="file-open"
          onClick={() => onOpenDiff(file.path)}
          className="flex-1 text-left hover:underline"
        >
          <span className="font-mono text-xs">{file.path}</span>
          <span className="block text-[11px] text-muted-foreground">
            {file.reason || "no signal"}
          </span>
        </button>
      </div>
      <div className="ml-6 mt-1 flex flex-wrap items-center gap-1">
        {deriveFileBadges(file).map((b) => (
          <Badge
            key={b.label}
            variant={badgeVariant(b.tone)}
            className="px-1.5 py-0 font-mono text-[9px] lowercase"
          >
            {b.label}
          </Badge>
        ))}
        <Button
          variant="ghost"
          size="sm"
          className="h-5 px-1.5 text-[9px] lowercase"
          onClick={() => setAdding((a) => !a)}
        >
          + note
        </Button>
      </div>
      {adding && (
        <div className="ml-6 mt-1 flex gap-1">
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
