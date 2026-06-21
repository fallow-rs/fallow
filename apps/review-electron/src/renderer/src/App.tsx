import { useCallback, useEffect, useState } from "react";
import { FileDiff, MonitorPlay, Camera, Sparkles, Loader2 } from "lucide-react";
import type { WalkthroughDocument } from "../../model/walkthrough";
import type { InspectorCard as InspectorCardData } from "../../main/inspect";
import { ReviewFocus } from "./components/ReviewFocus";
import { ClearedPanel } from "./components/ClearedPanel";
import { DecisionList } from "./components/DecisionList";
import { StageList } from "./components/StageList";
import { InspectorCard } from "./components/InspectorCard";
import { AgentPanel } from "./components/AgentPanel";
import { AnnotateCanvas } from "./components/AnnotateCanvas";
import { LiveApp } from "./components/LiveApp";
import { DiffView } from "./components/DiffView";
import { isViewed as readViewed, setViewed as writeViewed } from "./lib/viewed";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type RightMode = "diff" | "live" | "shot";

const MODES: { id: RightMode; label: string; icon: typeof FileDiff }[] = [
  { id: "diff", label: "diff", icon: FileDiff },
  { id: "live", label: "live", icon: MonitorPlay },
  { id: "shot", label: "screenshot", icon: Camera },
];

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
    <div className="grid h-screen grid-cols-[420px_1fr] overflow-hidden bg-background font-sans text-foreground">
      <aside className="flex min-h-0 flex-col border-r border-border bg-card">
        <header className="flex h-12 shrink-0 items-center justify-between gap-2 border-b border-border px-4">
          <div className="flex items-center gap-2">
            <Sparkles className="size-4 text-primary" />
            <h1 className="text-sm font-semibold lowercase">fallow review</h1>
          </div>
          <Button size="sm" disabled={loading} onClick={() => void load()}>
            {loading && <Loader2 className="size-3.5 animate-spin" />}
            {loading ? "reviewing" : "load review"}
          </Button>
        </header>

        <div className="min-h-0 flex-1 space-y-5 overflow-auto p-4">
          {error && (
            <p className="rounded-md border border-fallow-red/30 bg-fallow-red/10 p-2 text-xs text-fallow-red">
              {error}
            </p>
          )}
          {card && <InspectorCard card={card} />}
          <AgentPanel />
          {doc ? (
            <>
              <ReviewFocus focus={doc.focus} noteCount={noteCount} />
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
          ) : (
            !error && (
              <div className="flex flex-col items-center gap-2 py-16 text-center text-muted-foreground">
                <Sparkles className="size-6 opacity-40" />
                <p className="text-sm">load a review to see what to look at first</p>
              </div>
            )
          )}
        </div>
      </aside>

      <main className="flex min-h-0 flex-col overflow-hidden">
        <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
          <div className="inline-flex items-center gap-0.5 rounded-lg bg-muted p-0.5">
            {MODES.map(({ id, label, icon: Icon }) => {
              const disabled = id === "diff" && !diffFile;
              return (
                <button
                  key={id}
                  type="button"
                  data-testid={`mode-${id}`}
                  disabled={disabled}
                  onClick={() => setRightMode(id)}
                  className={cn(
                    "inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs lowercase transition-colors",
                    rightMode === id
                      ? "bg-background text-foreground shadow-sm"
                      : "text-muted-foreground hover:text-foreground",
                    disabled && "cursor-not-allowed opacity-40 hover:text-muted-foreground",
                  )}
                >
                  <Icon className="size-3.5" />
                  {label}
                </button>
              );
            })}
          </div>
          {rightMode === "diff" && diffFile && (
            <span className="truncate font-mono text-xs text-muted-foreground">{diffFile}</span>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-hidden">
          {rightMode === "diff" && diffFile ? (
            <DiffView file={diffFile} base={doc?.focus.baseRef ?? ""} />
          ) : rightMode === "live" ? (
            <LiveApp />
          ) : (
            <AnnotateCanvas />
          )}
        </div>
      </main>
    </div>
  );
};
