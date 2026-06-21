import { useCallback, useEffect, useState } from "react";
import type { WalkthroughDocument } from "../../model/walkthrough";
import type { InspectorCard as InspectorCardData } from "../../main/inspect";
import { ReviewFocus } from "./components/ReviewFocus";
import { ClearedPanel } from "./components/ClearedPanel";
import { DecisionList } from "./components/DecisionList";
import { StageList } from "./components/StageList";
import { InspectorCard } from "./components/InspectorCard";
import { AnnotateCanvas } from "./components/AnnotateCanvas";
import { LiveApp } from "./components/LiveApp";
import { isViewed as readViewed, setViewed as writeViewed } from "./lib/viewed";
import { theme } from "./theme";

export const App = () => {
  const [doc, setDoc] = useState<WalkthroughDocument | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [viewedTick, setViewedTick] = useState(0);
  const [noteCount, setNoteCount] = useState(0);
  const [card, setCard] = useState<InspectorCardData | null>(null);
  const [rightMode, setRightMode] = useState<"live" | "shot">("live");

  useEffect(() => {
    window.fallow.onInspectSelection(setCard);
  }, []);

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

  const onAddNote = useCallback((path: string, note: string) => {
    void window.fallow.appendFeed({
      target: { kind: "file_line", value: path },
      note,
      at: new Date().toISOString(),
    });
    setNoteCount((n) => n + 1);
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
        {noteCount > 0 && (
          <p style={{ fontSize: 11, color: theme.muted }}>
            {noteCount} note(s) sent to the agent feed
          </p>
        )}
        {error && (
          <p style={{ color: theme.danger, whiteSpace: "pre-wrap", fontSize: 12 }}>{error}</p>
        )}
        {card && <InspectorCard card={card} />}
        {doc && (
          <>
            <ReviewFocus focus={doc.focus} />
            <ClearedPanel cleared={doc.cleared} />
            <DecisionList decisions={doc.decisions} />
            <StageList
              stages={doc.stages}
              isViewed={isViewed}
              onToggleViewed={onToggleViewed}
              onAddNote={onAddNote}
            />
          </>
        )}
      </aside>
      <main style={{ overflow: "hidden", display: "grid", gridTemplateRows: "auto 1fr" }}>
        <div
          style={{ display: "flex", gap: 6, padding: 6, borderBottom: `1px solid ${theme.border}` }}
        >
          <button
            onClick={() => setRightMode("live")}
            disabled={rightMode === "live"}
            style={{ fontSize: 12 }}
          >
            Live app
          </button>
          <button
            onClick={() => setRightMode("shot")}
            disabled={rightMode === "shot"}
            style={{ fontSize: 12 }}
          >
            Screenshot URL
          </button>
        </div>
        {rightMode === "live" ? <LiveApp /> : <AnnotateCanvas />}
      </main>
    </div>
  );
};
