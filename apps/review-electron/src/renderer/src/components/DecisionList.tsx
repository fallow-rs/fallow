import type { Decision } from "../../../model/walkthrough";

export const DecisionList = ({ decisions }: { decisions: Decision[] }) => {
  if (decisions.length === 0) return null;
  return (
    <section className="mb-4">
      <h3 className="mb-1 text-xs font-medium lowercase">
        decisions (<span className="font-mono tabular-nums">{decisions.length}</span>)
      </h3>
      <ul className="list-disc pl-4 text-xs">
        {decisions.map((d) => (
          <li key={d.signalId}>{d.question || d.signalId}</li>
        ))}
      </ul>
    </section>
  );
};
