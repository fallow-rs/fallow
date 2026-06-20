import type { ReviewFocus as Focus } from "../../../model/walkthrough";
import { theme } from "../theme";

export const ReviewFocus = ({ focus }: { focus: Focus }) => (
  <header style={{ marginBottom: 16 }}>
    <div style={{ fontSize: 11, color: theme.muted }}>
      {focus.baseDescription} · {focus.baseRef.slice(0, 9)}
    </div>
    <h2 style={{ fontSize: 14, margin: "4px 0" }}>{focus.headline}</h2>
    <div style={{ fontSize: 11, color: theme.muted }}>
      {focus.changedFiles} files · risk {focus.riskClass} · effort {focus.reviewEffort}
    </div>
  </header>
);
