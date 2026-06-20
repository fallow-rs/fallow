import { useCallback, useState } from "react";
import type { WalkthroughDocument } from "../../model/walkthrough";
import { ReviewFocus } from "./components/ReviewFocus";
import { ClearedPanel } from "./components/ClearedPanel";
import { DecisionList } from "./components/DecisionList";
import { StageList } from "./components/StageList";
import { isViewed as readViewed, setViewed as writeViewed } from "./lib/viewed";
import { theme } from "./theme";

export const App = () => {
  const [doc, setDoc] = useState<WalkthroughDocument | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [viewedTick, setViewedTick] = useState(0);

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

  // viewedTick forces a fresh closure (and re-render) after each toggle.
  const isViewed = useCallback(
    (path: string) => viewedTick >= 0 && readViewed(window.localStorage, path),
    [viewedTick],
  );
  const onToggleViewed = useCallback((path: string) => {
    writeViewed(window.localStorage, path, !readViewed(window.localStorage, path));
    setViewedTick((t) => t + 1);
  }, []);

  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "440px 1fr",
        height: "100vh",
        fontFamily: "system-ui, sans-serif",
        background: theme.bg,
        color: theme.text,
      }}
    >
      <aside
        style={{
          borderRight: `1px solid ${theme.border}`,
          padding: 16,
          overflow: "auto",
          background: theme.panel,
        }}
      >
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: 12,
          }}
        >
          <h1 style={{ fontSize: 14, margin: 0 }}>Fallow Review</h1>
          <button onClick={() => void load()} disabled={loading} style={{ fontSize: 12 }}>
            {loading ? "Reviewing…" : "Load review"}
          </button>
        </div>
        {error && (
          <p style={{ color: theme.danger, whiteSpace: "pre-wrap", fontSize: 12 }}>{error}</p>
        )}
        {doc && (
          <>
            <ReviewFocus focus={doc.focus} />
            <ClearedPanel cleared={doc.cleared} />
            <DecisionList decisions={doc.decisions} />
            <StageList stages={doc.stages} isViewed={isViewed} onToggleViewed={onToggleViewed} />
          </>
        )}
      </aside>
      <main style={{ display: "grid", placeItems: "center", color: theme.muted }}>
        <span>app-under-review region (Phases 5-7)</span>
      </main>
    </div>
  );
};
