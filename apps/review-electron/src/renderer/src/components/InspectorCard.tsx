import type { InspectorCard as Card } from "../../../main/inspect";
import { theme } from "../theme";

export const InspectorCard = ({ card }: { card: Card }) => (
  <section
    style={{
      border: `1px solid ${theme.accent}`,
      borderRadius: 6,
      padding: 10,
      marginBottom: 12,
    }}
  >
    <div style={{ fontSize: 11, color: theme.muted }}>inspected</div>
    <div style={{ fontFamily: "ui-monospace, monospace", fontSize: 12 }}>
      {card.component ? `${card.component} · ` : ""}
      {card.file}:{card.line}
    </div>
    <ul style={{ margin: "6px 0 0", paddingLeft: 16, fontSize: 11, color: theme.muted }}>
      {card.facts.map((f, i) => (
        <li key={`${card.file}-${i}`}>{f}</li>
      ))}
    </ul>
  </section>
);
