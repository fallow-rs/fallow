import { useState } from "react";
import { ArrowDownToLine, FileText, Plus, ShieldAlert, TriangleAlert } from "lucide-react";
import type { WalkthroughFile } from "../../../model/walkthrough";
import { deriveFileSignal, type SignalTone } from "../lib/badges";
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

const FAN_IN_TONE: Record<SignalTone, string> = {
  hub: "text-fallow-amber",
  elevated: "text-foreground",
  muted: "text-muted-foreground",
};

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
  const signal = deriveFileSignal(file);

  return (
    <li
      className={cn(
        "group rounded-md px-2 py-1 hover:bg-accent/40",
        signal.deprioritized && "opacity-55",
        viewed && "opacity-40",
      )}
    >
      <div className="flex items-center gap-2">
        <Checkbox
          checked={viewed}
          onCheckedChange={() => onToggleViewed(file.path)}
          aria-label={`mark ${base} reviewed`}
        />
        <button
          type="button"
          data-testid="file-open"
          title={file.reason || undefined}
          onClick={() => onOpenDiff(file.path)}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          <FileText className="size-3.5 shrink-0 text-muted-foreground" />
          <span className="min-w-0 flex-1 truncate font-mono text-xs">
            <span className="text-muted-foreground">{dir}</span>
            <span className="text-foreground">{base}</span>
          </span>
        </button>
        <div className="flex shrink-0 items-center gap-1.5">
          {signal.security && (
            <ShieldAlert className="size-3.5 text-fallow-red" aria-label="security taint" />
          )}
          {signal.riskZone && (
            <TriangleAlert className="size-3.5 text-fallow-amber" aria-label="risk zone" />
          )}
          {signal.fanIn >= 2 && (
            <span
              title={`${signal.fanIn} importers depend on this`}
              className={cn(
                "inline-flex items-center gap-0.5 font-mono text-[10px] tabular-nums",
                FAN_IN_TONE[signal.fanInTone],
              )}
            >
              <ArrowDownToLine className="size-3" />
              {signal.fanIn}
            </span>
          )}
        </div>
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
