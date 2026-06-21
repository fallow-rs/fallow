import type { WalkthroughStage } from "../../../model/walkthrough";
import { FileRow } from "./FileRow";

type Props = {
  stages: WalkthroughStage[];
  isViewed: (path: string) => boolean;
  onToggleViewed: (path: string) => void;
  onAddNote: (path: string, note: string) => void;
  onOpenDiff: (path: string) => void;
};

export const StageList = ({ stages, isViewed, onToggleViewed, onAddNote, onOpenDiff }: Props) => (
  <section className="space-y-3">
    <h3 className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
      files to review
    </h3>
    {stages.map((stage) => (
      <div key={stage.moduleDir} className="space-y-0.5">
        <div className="flex items-center gap-2 px-2">
          <span className="font-mono text-[11px] tabular-nums text-muted-foreground">
            {stage.order + 1}
          </span>
          <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground">
            {stage.moduleDir}
          </span>
          <span className="font-mono text-[11px] tabular-nums text-muted-foreground">
            {stage.files.length}
          </span>
        </div>
        <ul className="space-y-0.5">
          {stage.files.map((f) => (
            <FileRow
              key={f.path}
              file={f}
              viewed={isViewed(f.path)}
              onToggleViewed={onToggleViewed}
              onAddNote={onAddNote}
              onOpenDiff={onOpenDiff}
            />
          ))}
        </ul>
      </div>
    ))}
  </section>
);
