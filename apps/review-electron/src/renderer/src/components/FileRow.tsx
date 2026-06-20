import type { WalkthroughFile } from "../../../model/walkthrough";
import { deriveFileBadges, type Tone } from "../lib/badges";
import { theme } from "../theme";

type Props = {
  file: WalkthroughFile;
  viewed: boolean;
  onToggleViewed: (path: string) => void;
};

const toneColor = (tone: Tone): string =>
  tone === "high" ? theme.high : tone === "info" ? theme.info : theme.muted;

export const FileRow = ({ file, viewed, onToggleViewed }: Props) => (
  <li
    style={{
      listStyle: "none",
      padding: "6px 0",
      borderBottom: `1px solid ${theme.border}`,
      opacity: viewed ? 0.45 : 1,
    }}
  >
    <label style={{ display: "flex", gap: 8, alignItems: "baseline", cursor: "pointer" }}>
      <input type="checkbox" checked={viewed} onChange={() => onToggleViewed(file.path)} />
      <span style={{ flex: 1 }}>
        <span style={{ fontFamily: "ui-monospace, monospace", fontSize: 12 }}>{file.path}</span>
        <span style={{ display: "block", color: theme.muted, fontSize: 11 }}>
          {file.reason || "no signal"}
        </span>
      </span>
    </label>
    <div style={{ display: "flex", gap: 4, marginTop: 2, marginLeft: 24, flexWrap: "wrap" }}>
      {deriveFileBadges(file).map((b) => (
        <span
          key={b.label}
          style={{
            fontSize: 9,
            padding: "1px 5px",
            borderRadius: 3,
            color: theme.bg,
            background: toneColor(b.tone),
          }}
        >
          {b.label}
        </span>
      ))}
    </div>
  </li>
);
