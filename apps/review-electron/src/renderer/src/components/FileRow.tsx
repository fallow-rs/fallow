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
  /** This file is the one currently shown in the diff pane. */
  active: boolean;
  /** Stage directory, stripped from the displayed path (it titles the group). */
  baseDir?: string;
  onToggleViewed: (path: string) => void;
  onAddNote: (path: string, note: string) => void;
  onOpenDiff: (path: string) => void;
};

const FAN_IN_TONE: Record<SignalTone, string> = {
  hub: "text-fallow-amber",
  elevated: "text-foreground",
  muted: "text-muted-foreground",
};

export const FileRow = ({
  file,
  viewed,
  active,
  baseDir,
  onToggleViewed,
  onAddNote,
  onOpenDiff,
}: Props) => {
  const [adding, setAdding] = useState(false);
  const [note, setNote] = useState("");

  const save = (): void => {
    if (note.trim()) onAddNote(file.path, note.trim());
    setNote("");
    setAdding(false);
  };

  // Drop the stage-dir prefix (the group header already shows it); always keep
  // the filename visible by letting only the residual dir shrink.
  const rel =
    baseDir && file.path.startsWith(`${baseDir}/`)
      ? file.path.slice(baseDir.length + 1)
      : file.path;
  const base = rel.split("/").pop() ?? rel;
  const dir = rel.slice(0, rel.length - base.length);
  const signal = deriveFileSignal(file);
  const title = file.reason ? `${file.path} · ${file.reason}` : file.path;

  return (
    <li
      className={cn(
        "group rounded-md transition-colors",
        active ? "bg-accent" : "hover:bg-accent/40",
        !active && signal.deprioritized && "opacity-55",
        !active && viewed && "opacity-40",
      )}
    >
      {/* A full-row click target sits under the content; only the checkbox and
          note button re-enable pointer events to capture their own clicks. */}
      <div className="relative px-2 py-1">
        <button
          type="button"
          data-testid="file-open"
          aria-label={`open ${base}`}
          title={title}
          onClick={() => onOpenDiff(file.path)}
          className="absolute inset-0 z-0 cursor-pointer rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring/60"
        />
        <div className="pointer-events-none relative z-10 flex items-center gap-2">
          <Checkbox
            checked={viewed}
            onCheckedChange={() => onToggleViewed(file.path)}
            aria-label={`mark ${base} reviewed`}
            className="pointer-events-auto"
          />
          <FileText
            className={cn(
              "size-3.5 shrink-0",
              active ? "text-foreground" : "text-muted-foreground",
            )}
          />
          <span className="flex min-w-0 flex-1 items-baseline font-mono text-xs">
            {dir && <span className="truncate text-muted-foreground">{dir}</span>}
            <span className="shrink-0 text-foreground">{base}</span>
          </span>
          <span className="flex shrink-0 items-center gap-1.5">
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
          </span>
          <Button
            variant="ghost"
            size="icon"
            className="pointer-events-auto size-6 shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100"
            aria-label="add note"
            onClick={() => setAdding((a) => !a)}
          >
            <Plus className="size-3.5" />
          </Button>
        </div>
      </div>
      {adding && (
        <div className="flex gap-1 pb-1.5 pl-9 pr-2">
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
