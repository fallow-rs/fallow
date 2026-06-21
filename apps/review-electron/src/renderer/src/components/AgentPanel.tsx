import { useEffect, useState } from "react";
import { Bot, Loader2, Play } from "lucide-react";
import type { AgentBackend } from "../../../main/backends";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/** Pick a coding-agent backend (codiff-style) and run a grounded agent review. */
export const AgentPanel = () => {
  const [backends, setBackends] = useState<AgentBackend[]>([]);
  const [selected, setSelected] = useState<string>("");
  const [status, setStatus] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  useEffect(() => {
    void Promise.all([window.fallow.listBackends(), window.fallow.getConfig()]).then(([b, cfg]) => {
      setBackends(b);
      setSelected(b.some((x) => x.id === cfg.agentBackend) ? cfg.agentBackend : (b[0]?.id ?? ""));
    });
  }, []);

  const run = async (): Promise<void> => {
    setRunning(true);
    setStatus("running agent…");
    try {
      const result = await window.fallow.runAgent(selected);
      setStatus(
        result.ok ? "agent judgments validated against the graph" : `error: ${result.error}`,
      );
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
      {status && <p className="text-[11px] text-muted-foreground">{status}</p>}
    </section>
  );
};
