import { useState } from "react";
import { ChevronRight } from "lucide-react";
import type { WalkthroughStage } from "../../../model/walkthrough";
import { FileRow } from "./FileRow";
import { cn } from "@/lib/utils";

type Props = {
  stages: WalkthroughStage[];
  isViewed: (path: string) => boolean;
  activePath: string | null;
  onToggleViewed: (path: string) => void;
  onAddNote: (path: string, note: string) => void;
  onOpenDiff: (path: string) => void;
};

const COLLAPSED_KEY = "fre:collapsed-stages";

/** Restore which stage groups were collapsed (keyed by module dir). */
const readCollapsed = (): Set<string> => {
  try {
    const raw = window.localStorage.getItem(COLLAPSED_KEY);
    return new Set(raw ? (JSON.parse(raw) as string[]) : []);
  } catch {
    return new Set();
  }
};

export const StageList = ({
  stages,
  isViewed,
  activePath,
  onToggleViewed,
  onAddNote,
  onOpenDiff,
}: Props) => {
  const [collapsed, setCollapsed] = useState<Set<string>>(readCollapsed);

  const toggle = (dir: string): void => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(dir)) next.delete(dir);
      else next.add(dir);
      window.localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...next]));
      return next;
    });
  };

  return (
    <section className="space-y-3">
      <h3 className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        files to review
      </h3>
      {stages.map((stage) => {
        const isCollapsed = collapsed.has(stage.moduleDir);
        return (
          <div key={stage.moduleDir} className="space-y-0.5">
            <button
              type="button"
              data-testid="stage-toggle"
              aria-expanded={!isCollapsed}
              onClick={() => toggle(stage.moduleDir)}
              className="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left font-mono text-[11px] tabular-nums text-muted-foreground outline-none transition-colors hover:bg-accent/40 focus-visible:ring-2 focus-visible:ring-ring/60"
            >
              <ChevronRight
                className={cn("size-3 shrink-0 transition-transform", !isCollapsed && "rotate-90")}
              />
              <span>{stage.order + 1}</span>
              <span className="min-w-0 flex-1 truncate text-left">{stage.moduleDir}</span>
              <span>{stage.files.length}</span>
            </button>
            {!isCollapsed && (
              <ul className="space-y-0.5">
                {stage.files.map((f) => (
                  <FileRow
                    key={f.path}
                    file={f}
                    viewed={isViewed(f.path)}
                    active={f.path === activePath}
                    baseDir={stage.moduleDir}
                    onToggleViewed={onToggleViewed}
                    onAddNote={onAddNote}
                    onOpenDiff={onOpenDiff}
                  />
                ))}
              </ul>
            )}
          </div>
        );
      })}
    </section>
  );
};
