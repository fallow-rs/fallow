import { useEffect, useState } from "react";
import { Bot, CheckCircle2, Loader2, Play, TriangleAlert } from "lucide-react";
import type { AgentBackend } from "../../../main/backends";
import type { ValidationEnvelope } from "../../../model/agent";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type Status = { kind: "running" | "ok" | "error"; text: string };

const STATUS_TONE: Record<Status["kind"], string> = {
  running: "text-muted-foreground",
  ok: "text-fallow-green",
  error: "text-fallow-red",
};

type AgentReport = { notesIncluded: number; validation: ValidationEnvelope };

/**
 * Pick a coding-agent backend (codiff-style) and run a grounded agent review.
 * Ownership of the validated result is lifted to {@link App} via `onResult`; the
 * accepted framing renders INLINE next to each decision (blueprint component 9),
 * so this panel keeps only the run controls and a run-level summary (notes sent,
 * stale warning, accepted/rejected counts), never the per-decision framing text.
 */
export const AgentPanel = ({
  onResult,
}: {
  onResult: (validation: ValidationEnvelope | null) => void;
}) => {
  const [backends, setBackends] = useState<AgentBackend[]>([]);
  const [selected, setSelected] = useState<string>("");
  const [status, setStatus] = useState<Status | null>(null);
  const [report, setReport] = useState<AgentReport | null>(null);
  const [running, setRunning] = useState(false);

  useEffect(() => {
    void Promise.all([window.fallow.listBackends(), window.fallow.getConfig()]).then(([b, cfg]) => {
      setBackends(b);
      setSelected(b.some((x) => x.id === cfg.agentBackend) ? cfg.agentBackend : (b[0]?.id ?? ""));
    });
  }, []);

  const run = async (): Promise<void> => {
    setRunning(true);
    setStatus({ kind: "running", text: "running agent…" });
    setReport(null);
    onResult(null);
    try {
      const result = await window.fallow.runAgent(selected);
      if (result.ok) {
        const validation = (result.validation ?? {}) as ValidationEnvelope;
        setStatus({ kind: "ok", text: "judgments validated against the graph" });
        setReport({ notesIncluded: result.notesIncluded, validation });
        onResult(validation);
      } else {
        setStatus({ kind: "error", text: result.error });
        onResult(null);
      }
    } finally {
      setRunning(false);
    }
  };

  if (backends.length === 0) return null;

  return (
    <section className="space-y-2.5 rounded-lg border border-border bg-muted/20 p-3">
      <div className="flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        <Bot className="size-3.5" />
        agent review
      </div>
      <div className="inline-flex items-center gap-0.5 rounded-lg bg-muted p-0.5">
        {backends.map((b) => (
          <button
            key={b.id}
            type="button"
            onClick={() => setSelected(b.id)}
            className={cn(
              "rounded-md px-2.5 py-1 text-[11px] lowercase outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring/60",
              selected === b.id
                ? "bg-background text-foreground shadow-sm"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {b.label}
          </button>
        ))}
      </div>
      <Button
        size="sm"
        className="w-full lowercase"
        disabled={running || !selected}
        onClick={() => void run()}
      >
        {running ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}
        {running ? "running" : "run agent review"}
      </Button>
      {status && (
        <p className={cn("flex items-start gap-1.5 text-[11px]", STATUS_TONE[status.kind])}>
          {status.kind === "running" ? (
            <Loader2 className="mt-px size-3 shrink-0 animate-spin" />
          ) : status.kind === "ok" ? (
            <CheckCircle2 className="mt-px size-3 shrink-0" />
          ) : (
            <TriangleAlert className="mt-px size-3 shrink-0" />
          )}
          <span className="min-w-0 break-words">{status.text}</span>
        </p>
      )}
      {report && <RunSummary report={report} />}
    </section>
  );
};

/**
 * Run-level summary only: notes sent, stale warning, and accepted/rejected
 * counts. The accepted framing TEXT now lives inline next to each decision in
 * DecisionList; here we point the reviewer at it rather than re-rendering it (so
 * they never have to mentally re-join framing to its decision).
 */
const RunSummary = ({ report }: { report: AgentReport }) => {
  const accepted = report.validation.accepted ?? [];
  const rejected = report.validation.rejected ?? [];
  return (
    <div className="space-y-1.5 text-[11px]">
      {report.notesIncluded > 0 && (
        <p className="text-muted-foreground">
          {report.notesIncluded} human note{report.notesIncluded === 1 ? "" : "s"} sent to the agent
          as context
        </p>
      )}
      {report.validation.stale && (
        <p className="text-fallow-amber">
          the tree moved since the guide was fetched; re-run to refresh
        </p>
      )}
      {accepted.length > 0 && (
        <p className="text-muted-foreground">
          {accepted.length} framing{accepted.length === 1 ? "" : "s"} shown inline with{" "}
          {accepted.length === 1 ? "its" : "their"} decision{accepted.length === 1 ? "" : "s"} below
        </p>
      )}
      {rejected.length > 0 && (
        <p className="text-fallow-red">
          {rejected.length} judgment{rejected.length === 1 ? "" : "s"} rejected (unanchored or stale)
        </p>
      )}
    </div>
  );
};
