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
  <div>
    {stages.map((stage) => (
      <section key={stage.moduleDir} className="mb-4">
        <h3 className="mb-1 font-mono text-xs text-muted-foreground">
          <span className="tabular-nums">{stage.order + 1}</span>. {stage.moduleDir}
        </h3>
        <ul className="m-0 p-0">
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
      </section>
    ))}
  </div>
);
