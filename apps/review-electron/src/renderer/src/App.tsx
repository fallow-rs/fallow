import { useState } from "react";
import type { WalkthroughDocument } from "../../model/walkthrough";

export const App = () => {
  const [doc, setDoc] = useState<WalkthroughDocument | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const load = async (): Promise<void> => {
    setError(null);
    setLoading(true);
    try {
      setDoc(await window.fallow.getReview());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "400px 1fr",
        height: "100vh",
        fontFamily: "system-ui, sans-serif",
      }}
    >
      <aside
        style={{
          borderRight: "1px solid #2a2521",
          padding: 16,
          overflow: "auto",
          background: "#16130f",
          color: "#e8e3da",
        }}
      >
        <h1 style={{ fontSize: 16, margin: "0 0 12px" }}>Fallow Review</h1>
        <button onClick={() => void load()} disabled={loading}>
          {loading ? "Reviewing…" : "Load review"}
        </button>
        {error && <p style={{ color: "#e5484d", whiteSpace: "pre-wrap" }}>{error}</p>}
        {doc && (
          <section style={{ marginTop: 16, fontSize: 13 }}>
            <p style={{ fontWeight: 600 }}>{doc.focus.headline}</p>
            <p>
              {doc.stages.length} stages · {doc.decisions.length} decisions
            </p>
            {doc.cleared.length > 0 && (
              <>
                <h2 style={{ fontSize: 13, marginBottom: 4 }}>Fallow already handled</h2>
                <ul style={{ margin: 0, paddingLeft: 18 }}>
                  {doc.cleared.map((c) => (
                    <li key={c.kind}>
                      {c.count} {c.label}
                    </li>
                  ))}
                </ul>
              </>
            )}
          </section>
        )}
      </aside>
      <main
        style={{
          background: "#0e0c0a",
          color: "#6b6356",
          display: "grid",
          placeItems: "center",
        }}
      >
        <span>app-under-review region (Phases 5-7)</span>
      </main>
    </div>
  );
};
