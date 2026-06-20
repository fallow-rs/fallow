import type { Decision } from "../../../model/walkthrough";
import { theme } from "../theme";

export const DecisionList = ({ decisions }: { decisions: Decision[] }) => {
  if (decisions.length === 0) return null;
  return (
    <section style={{ marginBottom: 16 }}>
      <h3 style={{ fontSize: 12, color: theme.accent, margin: "0 0 4px" }}>
        Decisions ({decisions.length})
      </h3>
      <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12 }}>
        {decisions.map((d) => (
          <li key={d.signalId}>{d.question || d.signalId}</li>
        ))}
      </ul>
    </section>
  );
};
