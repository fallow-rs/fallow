import { useCallback, useEffect, useState, type MouseEvent as ReactMouseEvent } from "react";
import {
  FileDiff,
  MonitorPlay,
  Camera,
  RefreshCw,
  Telescope,
  Loader2,
  TriangleAlert,
} from "lucide-react";
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
import { errorMessage } from "./lib/errors";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type RightMode = "diff" | "live" | "shot";

const MODES: { id: RightMode; label: string; icon: typeof FileDiff }[] = [
  { id: "diff", label: "diff", icon: FileDiff },
  { id: "live", label: "live", icon: MonitorPlay },
  { id: "shot", label: "screenshot", icon: Camera },
];

const SIDEBAR_MIN = 320;
const SIDEBAR_MAX = 760;
const SIDEBAR_DEFAULT = 420;
const SIDEBAR_KEY = "fre:sidebar-width";

/** Restore the persisted sidebar width, clamped to the allowed range. */
const readSidebarWidth = (): number => {
  const stored = Number(window.localStorage.getItem(SIDEBAR_KEY));
  return stored >= SIDEBAR_MIN && stored <= SIDEBAR_MAX ? stored : SIDEBAR_DEFAULT;
};

export const App = () => {
  const [doc, setDoc] = useState<WalkthroughDocument | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [viewedTick, setViewedTick] = useState(0);
  const [noteCount, setNoteCount] = useState(0);
  const [card, setCard] = useState<InspectorCardData | null>(null);
  const [rightMode, setRightMode] = useState<RightMode>("diff");
  const [diffFile, setDiffFile] = useState<string | null>(null);
  const [sidebarWidth, setSidebarWidth] = useState(readSidebarWidth);

  useEffect(() => {
    window.fallow.onInspectSelection(setCard);
  }, []);

  useEffect(() => {
    window.localStorage.setItem(SIDEBAR_KEY, String(sidebarWidth));
  }, [sidebarWidth]);

  // Drag the divider between the file list and the right pane to resize.
  const startResize = (e: ReactMouseEvent): void => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = sidebarWidth;
    const onMove = (ev: MouseEvent): void =>
      setSidebarWidth(
        Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startWidth + ev.clientX - startX)),
      );
    const onUp = (): void => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      document.body.style.removeProperty("cursor");
      document.body.style.removeProperty("user-select");
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  };

  const load = async (): Promise<void> => {
    setError(null);
    setLoading(true);
    try {
      setDoc(await window.fallow.getReview());
    } catch (e) {
      setError(errorMessage(e));
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
    <div
      className="grid h-screen overflow-hidden bg-background font-sans text-foreground"
      style={{ gridTemplateColumns: `${sidebarWidth}px 1fr` }}
    >
      <aside className="relative flex min-h-0 flex-col border-r border-border bg-card">
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label="resize the file list"
          title="drag to resize · double-click to reset"
          onMouseDown={startResize}
          onDoubleClick={() => setSidebarWidth(SIDEBAR_DEFAULT)}
          className="absolute inset-y-0 right-0 z-30 w-1.5 translate-x-1/2 cursor-col-resize transition-colors hover:bg-primary/40"
        />
        <header className="flex h-12 shrink-0 items-center justify-between gap-2 border-b border-border px-4">
          <div className="flex items-center gap-2">
            <Telescope className="size-4 text-primary" />
            <h1 className="text-sm font-semibold lowercase">fallow review</h1>
          </div>
          <Button size="sm" disabled={loading} onClick={() => void load()}>
            {loading && <Loader2 className="size-3.5 animate-spin" />}
            {loading ? "reviewing" : "load review"}
          </Button>
        </header>

        <div className="min-h-0 flex-1 space-y-5 overflow-auto p-4">
          {card && <InspectorCard card={card} />}
          {doc && <ReviewFocus focus={doc.focus} noteCount={noteCount} />}
          <AgentPanel />
          {doc ? (
            <>
              <ClearedPanel cleared={doc.cleared} />
              <DecisionList decisions={doc.decisions} onOpenDiff={onOpenDiff} />
              <StageList
                stages={doc.stages}
                isViewed={isViewed}
                activePath={diffFile}
                onToggleViewed={onToggleViewed}
                onAddNote={onAddNote}
                onOpenDiff={onOpenDiff}
              />
            </>
          ) : loading ? (
            <div className="flex flex-col items-center gap-2 py-16 text-center text-muted-foreground">
              <Loader2 className="size-6 animate-spin opacity-70" />
              <p className="text-sm">running fallow review…</p>
              <p className="text-[11px]">scoring blast radius and partitioning the diff</p>
            </div>
          ) : error ? (
            <div
              data-testid="review-error"
              className="flex flex-col items-center gap-3 py-16 text-center"
            >
              <div className="flex size-11 items-center justify-center rounded-full border border-fallow-red/30 bg-fallow-red/10">
                <TriangleAlert className="size-5 text-fallow-red" />
              </div>
              <div className="space-y-1">
                <p className="text-sm font-medium text-foreground">review failed</p>
                <p className="max-w-xs break-words text-xs text-muted-foreground">{error}</p>
              </div>
              <Button
                size="sm"
                variant="secondary"
                className="lowercase"
                onClick={() => void load()}
              >
                <RefreshCw className="size-3.5" />
                retry
              </Button>
            </div>
          ) : (
            <div className="flex flex-col items-center gap-2 py-16 text-center text-muted-foreground">
              <Telescope className="size-6 opacity-40" />
              <p className="text-sm">load a review to see what to look at first</p>
            </div>
          )}
        </div>
      </aside>

      <main className="flex min-h-0 flex-col overflow-hidden">
        <div className="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
          <div className="inline-flex items-center gap-0.5 rounded-lg bg-muted p-0.5">
            {MODES.map(({ id, label, icon: Icon }) => (
              <button
                key={id}
                type="button"
                data-testid={`mode-${id}`}
                onClick={() => setRightMode(id)}
                className={cn(
                  "inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs lowercase outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring/60",
                  rightMode === id
                    ? "bg-background text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                <Icon className="size-3.5" />
                {label}
              </button>
            ))}
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-hidden">
          {rightMode === "diff" ? (
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
