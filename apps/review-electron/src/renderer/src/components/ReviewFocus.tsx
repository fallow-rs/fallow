import type { ReviewFocus as Focus } from "../../../model/walkthrough";

export const ReviewFocus = ({ focus }: { focus: Focus }) => (
  <header className="mb-4">
    <div className="font-mono text-[11px] text-muted-foreground">
      {focus.baseDescription} · {focus.baseRef.slice(0, 9)}
    </div>
    <h2 className="my-1 text-sm lowercase">{focus.headline}</h2>
    <div className="text-[11px] text-muted-foreground">
      <span className="font-mono tabular-nums">{focus.changedFiles}</span> files · risk{" "}
      {focus.riskClass} · effort {focus.reviewEffort}
    </div>
  </header>
);
