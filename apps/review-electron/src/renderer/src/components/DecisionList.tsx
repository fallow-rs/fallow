import { GitBranchPlus } from "lucide-react";
import type { Decision } from "../../../model/walkthrough";

export const DecisionList = ({ decisions }: { decisions: Decision[] }) => {
  if (decisions.length === 0) return null;
  return (
    <section className="space-y-2">
      <h3 className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        decisions ({decisions.length})
      </h3>
      <ul className="space-y-1.5">
        {decisions.map((d) => (
          <li
            key={d.signalId}
            className="flex gap-2 rounded-md border border-border bg-muted/20 p-2 text-xs"
          >
            <GitBranchPlus className="size-3.5 shrink-0 text-fallow-amber" />
            <span>{d.question || d.signalId}</span>
          </li>
        ))}
      </ul>
    </section>
  );
};
