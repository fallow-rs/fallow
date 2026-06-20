import type { WalkthroughStage } from "../../../model/walkthrough";
import { FileRow } from "./FileRow";
import { theme } from "../theme";

type Props = {
  stages: WalkthroughStage[];
  isViewed: (path: string) => boolean;
  onToggleViewed: (path: string) => void;
};

export const StageList = ({ stages, isViewed, onToggleViewed }: Props) => (
  <div>
    {stages.map((stage) => (
      <section key={stage.moduleDir} style={{ marginBottom: 16 }}>
        <h3
          style={{
            fontSize: 12,
            color: theme.accent,
            margin: "0 0 4px",
            fontFamily: "ui-monospace, monospace",
          }}
        >
          {stage.order + 1}. {stage.moduleDir}
        </h3>
        <ul style={{ margin: 0, padding: 0 }}>
          {stage.files.map((f) => (
            <FileRow
              key={f.path}
              file={f}
              viewed={isViewed(f.path)}
              onToggleViewed={onToggleViewed}
            />
          ))}
        </ul>
      </section>
    ))}
  </div>
);
