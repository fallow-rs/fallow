import { useState } from "react";
import type { ClearedItem } from "../../../model/walkthrough";
import { theme } from "../theme";

export const ClearedPanel = ({ cleared }: { cleared: ClearedItem[] }) => {
  const [open, setOpen] = useState(false);
  if (cleared.length === 0) return null;
  const total = cleared.reduce((n, c) => n + c.count, 0);
  return (
    <section style={{ marginBottom: 16 }}>
      <button
        onClick={() => setOpen((o) => !o)}
        style={{
          background: "none",
          border: `1px solid ${theme.border}`,
          color: theme.muted,
          borderRadius: 4,
          padding: "4px 8px",
          cursor: "pointer",
          fontSize: 12,
        }}
      >
        {open ? "▾" : "▸"} Fallow already handled {total} technical items
      </button>
      {open && (
        <ul style={{ margin: "8px 0 0", paddingLeft: 18, color: theme.muted, fontSize: 12 }}>
          {cleared.map((c) => (
            <li key={c.kind}>
              {c.count} {c.label}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
};
