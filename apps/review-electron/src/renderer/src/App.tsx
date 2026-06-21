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
import { DiffView } from "./components/DiffView";
import { AgentPanel } from "./components/AgentPanel";
import { isViewed as readViewed, setViewed as writeViewed } from "./lib/viewed";
import { Button } from "@/components/ui/button";

type RightMode = "live" | "shot" | "diff";

export const App = () => {
  const [doc, setDoc] = useState<WalkthroughDocument | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [viewedTick, setViewedTick] = useState(0);
  const [noteCount, setNoteCount] = useState(0);
  const [card, setCard] = useState<InspectorCardData | null>(null);
  const [rightMode, setRightMode] = useState<RightMode>("live");
  const [diffFile, setDiffFile] = useState<string | null>(null);

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

  const onOpenDiff = useCallback((path: string) => {
    setDiffFile(path);
    setRightMode("diff");
  }, []);

  return (
    <div className="grid h-screen grid-cols-[440px_1fr] bg-background font-sans text-foreground">
      <aside className="flex flex-col overflow-auto border-r border-border bg-card p-4">
        <div className="mb-3 flex items-center justify-between">
          <h1 className="text-sm font-semibold lowercase">fallow review</h1>
          <Button size="sm" variant="secondary" disabled={loading} onClick={() => void load()}>
            {loading ? "reviewing…" : "load review"}
          </Button>
        </div>
        {noteCount > 0 && (
          <p className="mb-2 text-xs text-muted-foreground">
            <span className="font-mono tabular-nums">{noteCount}</span> note(s) sent to the agent
            feed
          </p>
        )}
        {error && <p className="mb-2 whitespace-pre-wrap text-xs text-destructive">{error}</p>}
        <AgentPanel />
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
              onOpenDiff={onOpenDiff}
            />
          </>
        )}
      </aside>
      <main className="grid grid-rows-[auto_1fr] overflow-hidden">
        <div className="flex items-center gap-1.5 border-b border-border p-1.5">
          <Button
            size="sm"
            variant={rightMode === "diff" ? "secondary" : "ghost"}
            disabled={!diffFile}
            onClick={() => setRightMode("diff")}
          >
            diff
          </Button>
          <Button
            size="sm"
            variant={rightMode === "live" ? "secondary" : "ghost"}
            onClick={() => setRightMode("live")}
          >
            live app
          </Button>
          <Button
            size="sm"
            variant={rightMode === "shot" ? "secondary" : "ghost"}
            onClick={() => setRightMode("shot")}
          >
            screenshot url
          </Button>
        </div>
        {rightMode === "diff" && diffFile ? (
          <DiffView file={diffFile} base={doc?.focus.baseRef ?? ""} />
        ) : rightMode === "live" ? (
          <LiveApp />
        ) : (
          <AnnotateCanvas />
        )}
      </main>
    </div>
  );
};
